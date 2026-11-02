//! Port of `AgentUi.java`: the server-backed agent popup. Closing the view
//! only closes its subscription, never the run.
//!
//! Threads: UI methods run on the main thread (`ui` handler); `request`
//! work runs on the single `requests` executor; `subscribe` starts a reader
//! thread for the event stream. `synchronized(incoming)` guards the
//! `incoming` event buffer shared by the reader thread and `flush`.
//!
//! HTTP goes through `notemaker_core::endpoint::HttpConnection`; `https://`
//! proxy URLs use rustls with the Android system CA store (ADR 0028).

use crate::android_consts::{
    COLOR_TRANSPARENT, COLOR_WHITE, GRAVITY_CENTER, INPUTTYPE_TYPE_CLASS_TEXT, INPUTTYPE_TYPE_TEXT_FLAG_CAP_SENTENCES, INPUTTYPE_TYPE_TEXT_FLAG_MULTI_LINE, R_LAYOUT_SIMPLE_SPINNER_DROPDOWN_ITEM, TOAST_LENGTH_LONG, VERSION_CODES_R,
    VIEW_FOCUS_DOWN, VIEW_GONE, VIEW_VISIBLE, WM_LAYOUT_PARAMS_FLAG_DIM_BEHIND, WM_LAYOUT_PARAMS_SOFT_INPUT_ADJUST_NOTHING, WM_LAYOUT_PARAMS_SOFT_INPUT_ADJUST_RESIZE,
};
use crate::concurrent::ExecutorService;
use crate::error::{AppError, AppResult};
use crate::field::Field;
use crate::jx::{self, JResult, JThread, Obj, Reply, WeakObj};
use crate::main_activity::MainActivity;
use crate::monitor::Monitor;
use crate::org_json::{JSONObject, JV};
use crate::tool_display::ToolDisplay;
use crate::ui;
use indexmap::IndexMap;
use notemaker_core::endpoint::{Disconnector, HttpConnection};
use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, Weak};

/// `private interface Work { JSONObject run() throws Exception; }` (runs on `requests`).
pub type Work = Box<dyn FnOnce() -> AppResult<JSONObject> + Send + 'static>;
/// `private interface Result { void accept(JSONObject result) throws Exception; }` (UI thread).
pub type Done = Box<dyn FnOnce(&Arc<AgentUi>, JSONObject) -> AppResult<()> + Send + 'static>;

/// `final class AgentUi`.
pub struct AgentUi {
    pub app: Weak<MainActivity>,
    /// `new Handler(Looper.getMainLooper())`.
    pub ui: Obj,
    /// `Executors.newSingleThreadExecutor()`.
    pub requests: ExecutorService,
    pub items: Field<IndexMap<String, Arc<Item>>>,
    /// `ArrayList<JSONObject> incoming`; guard with `incoming_lock` like
    /// Java's `synchronized(incoming)`.
    pub incoming: Field<Vec<JSONObject>>,
    pub incoming_lock: Monitor,
    pub dialog: Field<Option<Obj>>,
    pub body: Field<Option<Obj>>,
    pub messages: Field<Option<Obj>>,
    pub scroll: Field<Option<Obj>>,
    pub status: Field<Option<Obj>>,
    pub input: Field<Option<Obj>>,
    pub send: Field<Option<Obj>>,
    pub stop: Field<Option<Obj>>,
    pub base: Field<Option<String>>,
    pub proxy_token: Field<Option<String>>,
    pub chat: Field<Option<String>>,
    /// `volatile int generation`.
    pub generation: Field<i32>,
    /// `volatile HttpURLConnection stream` (its disconnect handle).
    pub stream: Field<Option<Disconnector>>,
    pub rendering: Field<bool>,
    pub busy: Field<bool>,
    pub pending: Field<Option<JSONObject>>,
    pub last_status: Field<String>,
}

type UiAction = Arc<dyn Fn(&Arc<AgentUi>) -> AppResult<()> + Send + Sync>;

fn action(f: impl Fn(&Arc<AgentUi>) -> AppResult<()> + Send + Sync + 'static) -> UiAction {
    Arc::new(f)
}
/// `String.valueOf(s)` of a nullable string.
fn s(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "null".into())
}
/// `value.replaceAll("/+$", "")`.
fn strip_slashes(value: &str) -> String {
    value.trim_end_matches('/').to_string()
}
fn set_enabled(view: &Obj, enabled: bool) -> JResult<()> {
    jcall!(view, "android/view/View", "setEnabled", "(Z)V", enabled)?;
    Ok(())
}
fn sdk_int() -> JResult<i32> {
    Ok(jsget!("android/os/Build$VERSION", "SDK_INT", "I")?.int())
}
fn string_adapter(context: &Obj, values: &[String]) -> JResult<Obj> {
    let refs: Vec<&str> = values.iter().map(String::as_str).collect();
    let array = jx::new_string_array(&refs)?;
    jnew!("android/widget/ArrayAdapter", "(Landroid/content/Context;I[Ljava/lang/Object;)V", context, R_LAYOUT_SIMPLE_SPINNER_DROPDOWN_ITEM, &array)
}
fn selected_item(spinner: &Obj) -> JResult<String> {
    let item = jcall!(spinner, "android/widget/AdapterView", "getSelectedItem", "()Ljava/lang/Object;")?.global();
    match item.and_then(|o| jx::to_string(o.as_obj())) {
        Some(s) => Ok(s),
        None => Err(jx::JavaError::new("java.lang.NullPointerException", "getSelectedItem() is null")),
    }
}
fn panic_error(p: Box<dyn std::any::Any + Send>) -> AppError {
    let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into());
    AppError::runtime(msg)
}
fn obj(args: &[(&str, JV)]) -> JSONObject {
    MainActivity::obj(args)
}

impl AgentUi {
    /// `AgentUi(MainActivity app)`.
    pub fn new(app: &Arc<MainActivity>) -> AppResult<Arc<AgentUi>> {
        Ok(Arc::new(AgentUi {
            app: Arc::downgrade(app),
            ui: ui::main_handler()?,
            requests: ExecutorService::single_thread(),
            items: Field::new(IndexMap::new()),
            incoming: Field::new(Vec::new()),
            incoming_lock: Monitor::new(),
            dialog: Field::new(None),
            body: Field::new(None),
            messages: Field::new(None),
            scroll: Field::new(None),
            status: Field::new(None),
            input: Field::new(None),
            send: Field::new(None),
            stop: Field::new(None),
            base: Field::new(None),
            proxy_token: Field::new(None),
            chat: Field::new(None),
            generation: Field::new(0),
            stream: Field::new(None),
            rendering: Field::new(false),
            busy: Field::new(false),
            pending: Field::new(None),
            last_status: Field::new(String::new()),
        }))
    }
    pub fn app(&self) -> Arc<MainActivity> {
        self.app.upgrade().expect("MainActivity dropped")
    }
    pub fn context(&self) -> Obj {
        self.app().this.clone()
    }
    /// `private int dp(int v)`.
    pub fn dp(&self, v: i32) -> AppResult<i32> {
        Ok(crate::ui::dp(&self.context(), v as f32)?)
    }
    /// `private LinearLayout column()`.
    pub fn column(&self) -> AppResult<Obj> {
        Ok(crate::ui::column(&self.context())?)
    }
    fn body(&self) -> Obj {
        self.body.get().expect("body")
    }
    fn status_view(&self) -> Obj {
        self.status.get().expect("status")
    }
    fn post(&self, f: impl FnOnce() -> AppResult<()> + Send + 'static) {
        let r = jx::try_once_runnable(move || f().map_err(Into::into));
        if let Err(e) = ui::post(&self.ui, &r) {
            jx::log::e_err("NoteMaker", "Handler.post failed", &e);
        }
    }
    /// `private TextView text(String value, int size)`.
    pub fn text(&self, value: &str, size: i32) -> AppResult<Obj> {
        let v = ui::text_view(&self.context())?;
        ui::set_text(&v, value)?;
        jcall!(v, "android/widget/TextView", "setTextSize", "(F)V", size as f32)?;
        jcall!(v, "android/widget/TextView", "setTextColor", "(I)V", 0xff202521u32 as i32)?;
        ui::set_padding(&v, self.dp(4)?, self.dp(6)?, self.dp(4)?, self.dp(6)?)?;
        Ok(v)
    }
    /// `private Button button(String title, Runnable action)`.
    pub fn button(self: &Arc<Self>, title: &str, action: Arc<dyn Fn(&Arc<AgentUi>) -> AppResult<()> + Send + Sync>) -> AppResult<Obj> {
        let v = jnew!("android/widget/Button", "(Landroid/content/Context;)V", &self.context())?;
        ui::set_text(&v, title)?;
        jcall!(v, "android/widget/TextView", "setAllCaps", "(Z)V", false)?;
        ui::set_content_description(&v, title)?;
        let weak = Arc::downgrade(self);
        ui::on_click(&v, move |_| match weak.upgrade() {
            Some(me) => action(&me).map_err(Into::into),
            None => Ok(()),
        })?;
        Ok(v)
    }
    /// `private void cancelStream()`.
    pub fn cancel_stream(&self) {
        self.generation.set(self.generation.get() + 1);
        self.rendering.set(false);
        let connection = self.stream.replace(None);
        if let Some(connection) = connection {
            self.requests.execute(move || connection.disconnect());
        }
        let _g = self.incoming_lock.enter();
        self.incoming.with_mut(|i| i.clear());
    }
    /// `private String draftKey()`.
    pub fn draft_key(&self) -> String {
        format!("agent-draft:{}:{}", s(&self.base.get()), s(&self.chat.get()))
    }
    /// `private String pendingKey()`.
    pub fn pending_key(&self) -> String {
        format!("agent-pending:{}:{}", s(&self.base.get()), s(&self.chat.get()))
    }
    /// `private void saveDraft()`.
    pub fn save_draft(&self) -> AppResult<()> {
        if let (Some(_), Some(input)) = (self.chat.get(), self.input.get()) {
            let context = self.context();
            if self.pending.get().is_some() && !ui::pref_contains(&context, &self.pending_key())? {
                return Ok(());
            }
            let editor = ui::edit(&context)?;
            ui::put_string(&editor, &self.draft_key(), &ui::text_of(&input)?)?;
            ui::apply(&editor)?;
        }
        Ok(())
    }
    /// `void dismiss()`.
    pub fn dismiss(&self) -> AppResult<()> {
        if let Some(dialog) = self.dialog.get() {
            jcall!(dialog, "android/app/Dialog", "dismiss", "()V")?;
        }
        Ok(())
    }
    /// `boolean showing()`.
    pub fn showing(&self) -> AppResult<bool> {
        Ok(match self.dialog.get() {
            Some(dialog) => jcall!(dialog, "android/app/Dialog", "isShowing", "()Z")?.bool(),
            None => false,
        })
    }
    /// `void destroy()`.
    pub fn destroy(&self) -> AppResult<()> {
        self.dismiss()?;
        self.requests.shutdown_now();
        Ok(())
    }
    fn configure_proxy(&self, app: &Arc<MainActivity>) -> AppResult<()> {
        let proxy_url = app.proxy_url.get();
        if !proxy_url.is_empty() {
            self.base.set(Some(strip_slashes(&proxy_url)));
            self.proxy_token.set(Some(app.proxy_token.get()));
        } else {
            crate::replica_service::ensure(&app.this)?;
            let descriptor = app.files_dir()?.join("proxy.json");
            let bytes = std::fs::read(&descriptor).map_err(|e| AppError::java("java.nio.file.NoSuchFileException", if e.kind() == std::io::ErrorKind::NotFound { descriptor.display().to_string() } else { e.to_string() }))?;
            let proxy = JSONObject::parse(&String::from_utf8_lossy(&bytes))?;
            self.base.set(Some(strip_slashes(&proxy.get_string("url")?)));
            self.proxy_token.set(Some(proxy.get_string("token")?));
        }
        Ok(())
    }
    /// `void show()`.
    pub fn show(self: &Arc<Self>) -> AppResult<()> {
        if self.showing()? {
            return Ok(());
        }
        let app = self.app();
        if let Err(e) = self.configure_proxy(&app) {
            ui::toast(&app.this, &format!("Cannot connect to the configured proxy: {}", e.message_or_null()), TOAST_LENGTH_LONG)?;
            return Ok(());
        }
        let ctx = app.this.clone();
        let dialog = jnew!("android/app/Dialog", "(Landroid/content/Context;)V", &ctx)?;
        self.dialog.set(Some(dialog.clone()));
        let title = jx::jstr("Agent");
        jcall!(dialog, "android/app/Dialog", "setTitle", "(Ljava/lang/CharSequence;)V", &title)?;
        let body = self.column()?;
        self.body.set(Some(body.clone()));
        ui::set_padding(&body, self.dp(16)?, self.dp(12)?, self.dp(16)?, self.dp(12)?)?;
        ui::set_background_color(&body, COLOR_WHITE)?;
        // A full-window transparent host receives IME insets even on BOOX. Only
        // the centered white panel is visible, and it fits the remaining space.
        let frame = ui::frame_layout(&ctx)?;
        let panel = body.clone();
        jcall!(panel, "android/view/View", "setClickable", "(Z)V", true)?;
        ui::set_padding(&frame, self.dp(12)?, self.dp(12)?, self.dp(12)?, self.dp(12)?)?;
        ui::add_view_params(&frame, &panel, &ui::frame_params_gravity(-1, -1, GRAVITY_CENTER)?)?;
        let weak = Arc::downgrade(self);
        ui::on_click(&frame, move |_| match weak.upgrade() {
            Some(me) => me.dismiss().map_err(Into::into),
            None => Ok(()),
        })?;
        let (frame_ref, panel_ref) = (WeakObj::new(&frame), WeakObj::new(&panel));
        let weak = Arc::downgrade(self);
        let layout = jx::callback(move |_, _| {
            if let (Some(me), Some(frame), Some(panel)) = (weak.upgrade(), frame_ref.upgrade(), panel_ref.upgrade()) {
                me.resize_popup(&frame, &panel)?;
            }
            Ok(Reply::Void)
        });
        jcall!(frame, "android/view/View", "addOnLayoutChangeListener", "(Landroid/view/View$OnLayoutChangeListener;)V", &layout)?;
        if sdk_int()? >= 30 {
            let (frame_ref, panel_ref) = (WeakObj::new(&frame), WeakObj::new(&panel));
            let weak = Arc::downgrade(self);
            let insets_listener = jx::callback(move |_, args| {
                let insets = jx::global(&args[1]);
                if let (Some(me), Some(frame), Some(panel), Some(insets)) = (weak.upgrade(), frame_ref.upgrade(), panel_ref.upgrade(), insets.clone()) {
                    let types = jstatic!("android/view/WindowInsets$Type", "systemBars", "()I")?.int() | jstatic!("android/view/WindowInsets$Type", "displayCutout", "()I")?.int() | jstatic!("android/view/WindowInsets$Type", "ime", "()I")?.int();
                    let safe = jcall!(insets, "android/view/WindowInsets", "getInsets", "(I)Landroid/graphics/Insets;", types)?.some();
                    let left = jget!(safe, "android/graphics/Insets", "left", "I")?.int();
                    let top = jget!(safe, "android/graphics/Insets", "top", "I")?.int();
                    let right = jget!(safe, "android/graphics/Insets", "right", "I")?.int();
                    let bottom = jget!(safe, "android/graphics/Insets", "bottom", "I")?.int();
                    let pad = me.dp(12)?;
                    ui::set_padding(&frame, left + pad, top + pad, right + pad, bottom + pad)?;
                    me.resize_popup(&frame, &panel)?;
                }
                Ok(Reply::Obj(insets))
            });
            jcall!(frame, "android/view/View", "setOnApplyWindowInsetsListener", "(Landroid/view/View$OnApplyWindowInsetsListener;)V", &insets_listener)?;
        }
        jcall!(dialog, "android/app/Dialog", "setContentView", "(Landroid/view/View;)V", &frame)?;
        let weak = Arc::downgrade(self);
        let dismissed = jx::callback(move |_, _| {
            if let Some(me) = weak.upgrade() {
                me.save_draft()?;
                me.cancel_stream();
                me.dialog.set(None);
            }
            Ok(Reply::Void)
        });
        jcall!(dialog, "android/app/Dialog", "setOnDismissListener", "(Landroid/content/DialogInterface$OnDismissListener;)V", &dismissed)?;
        jcall!(dialog, "android/app/Dialog", "show", "()V")?;
        let window = jcall!(dialog, "android/app/Dialog", "getWindow", "()Landroid/view/Window;")?.global();
        if let Some(window) = window {
            let background = jnew!("android/graphics/drawable/ColorDrawable", "(I)V", COLOR_TRANSPARENT)?;
            jcall!(window, "android/view/Window", "setBackgroundDrawable", "(Landroid/graphics/drawable/Drawable;)V", &background)?;
            jcall!(window, "android/view/Window", "addFlags", "(I)V", WM_LAYOUT_PARAMS_FLAG_DIM_BEHIND)?;
            jcall!(window, "android/view/Window", "setDimAmount", "(F)V", 0.32f32)?;
            jcall!(window, "android/view/Window", "setLayout", "(II)V", -1, -1)?;
            if sdk_int()? >= VERSION_CODES_R {
                jcall!(window, "android/view/Window", "setDecorFitsSystemWindows", "(Z)V", false)?;
                jcall!(window, "android/view/Window", "setSoftInputMode", "(I)V", WM_LAYOUT_PARAMS_SOFT_INPUT_ADJUST_NOTHING)?;
            } else {
                jcall!(window, "android/view/Window", "setSoftInputMode", "(I)V", WM_LAYOUT_PARAMS_SOFT_INPUT_ADJUST_RESIZE)?;
            }
        }
        jcall!(frame, "android/view/View", "requestApplyInsets", "()V")?;
        self.hub(0, true)
    }
    /// `private void resizePopup(FrameLayout frame, LinearLayout panel)`.
    pub fn resize_popup(&self, frame: &Obj, panel: &Obj) -> AppResult<()> {
        let frame_height = ui::height(frame)?;
        if frame_height == 0 {
            return Ok(());
        }
        let pad = |name: &str| -> JResult<i32> {
            Ok(match name {
                "left" => jcall!(frame, "android/view/View", "getPaddingLeft", "()I")?.int(),
                "right" => jcall!(frame, "android/view/View", "getPaddingRight", "()I")?.int(),
                "top" => jcall!(frame, "android/view/View", "getPaddingTop", "()I")?.int(),
                _ => jcall!(frame, "android/view/View", "getPaddingBottom", "()I")?.int(),
            })
        };
        let width = 1.max(self.dp(640)?.min(ui::width(frame)? - pad("left")? - pad("right")?));
        let height = 1.max(((frame_height as f64 * 0.85) as i32).min(frame_height - pad("top")? - pad("bottom")?));
        let params = jcall!(panel, "android/view/View", "getLayoutParams", "()Landroid/view/ViewGroup$LayoutParams;")?.some();
        let current_width = jget!(params, "android/view/ViewGroup$LayoutParams", "width", "I")?.int();
        let current_height = jget!(params, "android/view/ViewGroup$LayoutParams", "height", "I")?.int();
        if current_width != width || current_height != height {
            jset!(params, "android/view/ViewGroup$LayoutParams", "width", "I", width)?;
            jset!(params, "android/view/ViewGroup$LayoutParams", "height", "I", height)?;
            jcall!(panel, "android/view/View", "setLayoutParams", "(Landroid/view/ViewGroup$LayoutParams;)V", &params)?;
        }
        Ok(())
    }
    /// `private void clear(String heading)`.
    pub fn clear(self: &Arc<Self>, heading: &str) -> AppResult<()> {
        self.save_draft()?;
        self.cancel_stream();
        self.chat.set(None);
        self.input.set(None);
        self.pending.set(None);
        let body = self.body();
        jcall!(body, "android/view/ViewGroup", "removeAllViews", "()V")?;
        let row = jnew!("android/widget/LinearLayout", "(Landroid/content/Context;)V", &self.context())?;
        ui::add_view_params(&row, &self.text(heading, 22)?, &ui::linear_params_weight(0, -2, 1.0)?)?;
        ui::add_view(&row, &self.button("Close", action(|me| me.dismiss()))?)?;
        ui::add_view(&body, &row)?;
        let status = self.text("", 13)?;
        self.status.set(Some(status.clone()));
        ui::add_view(&body, &status)?;
        Ok(())
    }
    /// `private void request(Work work, Result done)`.
    pub fn request(self: &Arc<Self>, work: Work, done: Done) {
        let token = self.generation.get();
        let me = self.clone();
        self.requests.execute(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|p| Err(panic_error(p)));
            let ui_me = me.clone();
            match result {
                Ok(result) => me.post(move || {
                    let me = ui_me;
                    if token == me.generation.get() && me.dialog.get().is_some() {
                        if let Err(e) = done(&me, result) {
                            ui::set_text(&me.status_view(), &e.message().unwrap_or_default())?;
                        }
                    }
                    Ok(())
                }),
                Err(e) => me.post(move || {
                    let me = ui_me;
                    if token == me.generation.get() && me.dialog.get().is_some() {
                        ui::set_text(&me.status_view(), &format!("Connection/request failed: {}", e.message_or_null()))?;
                        if let (Some(send), Some(_)) = (me.send.get(), me.chat.get()) {
                            set_enabled(&send, true)?;
                            if me.pending.get().is_some() {
                                ui::set_text(&send, "Retry send")?;
                            }
                        }
                    }
                    Ok(())
                }),
            }
        });
    }
    /// `private JSONObject http(String url, JSONObject data) throws Exception`
    /// (`HttpURLConnection` → `notemaker_core::endpoint::HttpConnection`).
    pub fn http(&self, url: &str, data: Option<&JSONObject>) -> AppResult<JSONObject> {
        let mut c = HttpConnection::new(url)?;
        c.set_connect_timeout(5000);
        c.set_read_timeout(10000);
        c.set_request_property("Authorization", &format!("Bearer {}", s(&self.proxy_token.get())));
        let result = (|| -> AppResult<JSONObject> {
            if let Some(data) = data {
                c.set_request_method("POST");
                c.set_request_property("Content-Type", "application/json");
                c.write_body(data.to_string().into_bytes());
            }
            let code = c.response_code()?;
            let source = if code < 400 { Some(c.input_stream()?) } else { c.error_stream()? };
            let Some(mut source) = source else {
                return Err(AppError::io(format!("HTTP {code}")));
            };
            let mut bytes = Vec::new();
            source.read_to_end(&mut bytes)?;
            let result = JSONObject::parse(&String::from_utf8_lossy(&bytes))?;
            if code >= 400 {
                return Err(AppError::io(result.opt_string_or("error", &format!("HTTP {code}"))));
            }
            Ok(result)
        })();
        c.disconnect();
        result
    }
    /// `private void hub(int offset, boolean recent)`.
    pub fn hub(self: &Arc<Self>, offset: i32, recent: bool) -> AppResult<()> {
        self.clear(if recent { "Agent" } else { "All chats" })?;
        let body = self.body();
        ui::add_view(&body, &self.button("New chat", action(|me| me.new_chat()))?)?;
        let list_scroll = jnew!("android/widget/ScrollView", "(Landroid/content/Context;)V", &self.context())?;
        let list = self.column()?;
        ui::add_view(&list_scroll, &list)?;
        ui::add_view_params(&body, &list_scroll, &ui::linear_params_weight(-1, 0, 1.0)?)?;
        ui::set_text(&self.status_view(), if recent { "Five most recent chats" } else { "Chat history" })?;
        let url = format!("{}/api/chats?limit={}&offset={}", s(&self.base.get()), if recent { 5 } else { 20 }, offset);
        let me = self.clone();
        self.request(
            Box::new(move || me.http(&url, None)),
            Box::new(move |me, result| {
                let chats = result.get_json_array("chats")?;
                if chats.length() == 0 {
                    ui::add_view(&list, &me.text("No chats yet", 16)?)?;
                }
                for i in 0..chats.length() {
                    let item = chats.get_json_object(i)?;
                    let title = format!("{} · {}", item.get_string("title")?, item.get_string("status")?);
                    ui::add_view(&list, &me.button(&title, action(move |me| me.conversation(&item)))?)?;
                }
                let body = me.body();
                if recent {
                    ui::add_view(&body, &me.button("More chats", action(|me| me.hub(0, false)))?)?;
                } else {
                    if offset > 0 {
                        ui::add_view(&body, &me.button("Previous chats", action(move |me| me.hub(0.max(offset - 20), false)))?)?;
                    }
                    if result.opt_boolean("more") {
                        ui::add_view(&body, &me.button("Next chats", action(move |me| me.hub(offset + 20, false)))?)?;
                    }
                    ui::add_view(&body, &me.button("Recent chats", action(|me| me.hub(0, true)))?)?;
                }
                Ok(())
            }),
        );
        ui::add_view(&body, &self.button("Refresh", action(move |me| me.hub(offset, recent)))?)?;
        Ok(())
    }
    /// `private Spinner spinner(String description, String[] values)` (listener `AgentUi$1`).
    pub fn spinner(&self, description: &str, values: &[String]) -> AppResult<Obj> {
        let ctx = self.context();
        let v = jnew!("android/widget/Spinner", "(Landroid/content/Context;)V", &ctx)?;
        ui::set_content_description(&v, description)?;
        let adapter = string_adapter(&ctx, values)?;
        jcall!(v, "android/widget/Spinner", "setAdapter", "(Landroid/widget/SpinnerAdapter;)V", &adapter)?;
        let body = self.body();
        ui::add_view(&body, &self.text(description, 14)?)?;
        ui::add_view(&body, &v)?;
        Ok(v)
    }
    /// `private void newChat()`.
    pub fn new_chat(self: &Arc<Self>) -> AppResult<()> {
        self.clear("New chat")?;
        ui::set_text(&self.status_view(), "Loading available agents…")?;
        let url = format!("{}/api/chats/options", s(&self.base.get()));
        let me = self.clone();
        self.request(
            Box::new(move || me.http(&url, None)),
            Box::new(move |me, result| {
                ui::set_text(&me.status_view(), "Choose an agent for this conversation")?;
                let harnesses = result.get_json_array("harnesses")?;
                let models = result.get_json_array("models")?;
                let mut harness_names = Vec::with_capacity(harnesses.length());
                for i in 0..harnesses.length() {
                    harness_names.push(harnesses.get_string(i)?);
                }
                let mut model_names = Vec::with_capacity(models.length());
                for i in 0..models.length() {
                    model_names.push(format!("{} / {}", models.get_json_object(i)?.get_string("provider")?, models.get_json_object(i)?.get_string("model")?));
                }
                let harness = me.spinner("Harness", &harness_names)?;
                let model = me.spinner("Model", &model_names)?;
                let effort = me.spinner("Effort", &["medium".to_string()])?;
                let (effort_ref, choices_models, weak) = (WeakObj::new(&effort), models.clone(), Arc::downgrade(me));
                let selected_listener = jx::callback(move |method, args| {
                    if method != "onItemSelected(AdapterView,View,int,long)" {
                        return Ok(Reply::Void);
                    }
                    let (Some(me), Some(effort)) = (weak.upgrade(), effort_ref.upgrade()) else { return Ok(Reply::Void) };
                    let position = jx::unbox_int(&args[2]);
                    let choices = choices_models
                        .opt_json_object(position as usize)
                        .and_then(|m| m.opt_json_array("efforts"))
                        .ok_or_else(|| jx::JavaError::new("java.lang.NullPointerException", "efforts"))?;
                    let mut names = Vec::with_capacity(choices.length());
                    let mut selected = 0;
                    for i in 0..choices.length() {
                        names.push(choices.opt_string(i));
                        if names[i] == "medium" {
                            selected = i as i32;
                        }
                    }
                    let adapter = string_adapter(&me.context(), &names)?;
                    jcall!(effort, "android/widget/Spinner", "setAdapter", "(Landroid/widget/SpinnerAdapter;)V", &adapter)?;
                    jcall!(effort, "android/widget/AbsSpinner", "setSelection", "(I)V", selected)?;
                    Ok(Reply::Void)
                });
                jcall!(model, "android/widget/AdapterView", "setOnItemSelectedListener", "(Landroid/widget/AdapterView$OnItemSelectedListener;)V", &selected_listener)?;
                let identity = notemaker_core::ids::random_uuid();
                let (harness_ref, model_ref, effort_ref) = (WeakObj::new(&harness), WeakObj::new(&model), WeakObj::new(&effort));
                let create = me.button(
                    "Start chat",
                    action(move |me| {
                        let (Some(harness), Some(model), Some(effort)) = (harness_ref.upgrade(), model_ref.upgrade(), effort_ref.upgrade()) else { return Ok(()) };
                        let position = jcall!(model, "android/widget/AdapterView", "getSelectedItemPosition", "()I")?.int();
                        let selected = models.opt_json_object(position.max(0) as usize).filter(|_| position >= 0).ok_or_else(|| AppError::java("java.lang.NullPointerException", "selected model"))?;
                        let config = obj(&[
                            ("id", JV::from(identity.as_str())),
                            ("harness", JV::from(selected_item(&harness)?)),
                            ("provider", JV::from(selected.opt_string("provider"))),
                            ("model", JV::from(selected.opt_string("model"))),
                            ("effort", JV::from(selected_item(&effort)?)),
                        ]);
                        let target = format!("{}/api/chats", s(&me.base.get()));
                        let worker = me.clone();
                        me.request(Box::new(move || worker.http(&target, Some(&config))), Box::new(|me, result| me.conversation(&result)));
                        Ok(())
                    }),
                )?;
                ui::add_view(&me.body(), &create)?;
                Ok(())
            }),
        );
        ui::add_view(&self.body(), &self.button("Back to chats", action(|me| me.hub(0, true)))?)?;
        Ok(())
    }
    /// `private void conversation(JSONObject data)`.
    pub fn conversation(self: &Arc<Self>, data: &JSONObject) -> AppResult<()> {
        self.clear(&data.opt_string_or("title", "Chat"))?;
        self.chat.set(Some(data.opt_string("id")));
        self.items.with_mut(|i| i.clear());
        let body = self.body();
        let ctx = self.context();
        ui::add_view(&body, &self.button("Back to chats", action(|me| me.hub(0, true)))?)?;
        ui::add_view(&body, &self.text(&format!("{} / {} / {}", data.opt_string("harness"), data.opt_string("model"), data.opt_string("effort")), 12)?)?;
        let scroll = jnew!("android/widget/ScrollView", "(Landroid/content/Context;)V", &ctx)?;
        let messages = self.column()?;
        ui::add_view(&scroll, &messages)?;
        self.scroll.set(Some(scroll.clone()));
        self.messages.set(Some(messages));
        ui::add_view_params(&body, &scroll, &ui::linear_params_weight(-1, 0, 1.0)?)?;
        let input = jnew!("android/widget/EditText", "(Landroid/content/Context;)V", &ctx)?;
        let hint = jx::jstr("Message agent");
        jcall!(input, "android/widget/TextView", "setHint", "(Ljava/lang/CharSequence;)V", &hint)?;
        ui::set_content_description(&input, "Message agent")?;
        jcall!(input, "android/widget/TextView", "setMinLines", "(I)V", 1)?;
        jcall!(input, "android/widget/TextView", "setMaxLines", "(I)V", 4)?;
        jcall!(input, "android/widget/TextView", "setInputType", "(I)V", INPUTTYPE_TYPE_CLASS_TEXT | INPUTTYPE_TYPE_TEXT_FLAG_MULTI_LINE | INPUTTYPE_TYPE_TEXT_FLAG_CAP_SENTENCES)?;
        ui::set_text(&input, &ui::pref_string(&ctx, &self.draft_key(), Some(""))?.unwrap_or_default())?;
        ui::add_view(&body, &input)?;
        self.input.set(Some(input.clone()));
        if let Ok(Some(saved)) = ui::pref_string(&ctx, &self.pending_key(), Some("")) {
            if !saved.is_empty() {
                if let Ok(pending) = JSONObject::parse(&saved) {
                    self.pending.set(Some(pending));
                }
            }
        }
        let actions = jnew!("android/widget/LinearLayout", "(Landroid/content/Context;)V", &ctx)?;
        let send = self.button("Send", action(|me| me.send()))?;
        let stop = self.button("Stop", action(|me| me.stop()))?;
        self.send.set(Some(send.clone()));
        self.stop.set(Some(stop.clone()));
        ui::add_view_params(&actions, &send, &ui::linear_params_weight(0, -2, 1.0)?)?;
        ui::add_view_params(&actions, &stop, &ui::linear_params_weight(0, -2, 1.0)?)?;
        ui::add_view(&body, &actions)?;
        self.set_status(&data.opt_string("status"), &data.opt_string("status"))?;
        if let Some(pending) = self.pending.get() {
            ui::set_text(&input, &pending.opt_string("text"))?;
            ui::set_text(&send, "Retry send")?;
            set_enabled(&send, true)?;
        }
        self.subscribe(self.generation.get(), &s(&self.base.get()), &s(&self.chat.get()));
        Ok(())
    }
    /// `private void setStatus(String state, String detail)`.
    pub fn set_status(&self, state: &str, detail: &str) -> AppResult<()> {
        let busy = state == "running";
        self.busy.set(busy);
        self.last_status.set(detail.to_string());
        ui::set_text(&self.status_view(), detail)?;
        set_enabled(&self.stop.get().expect("stop"), busy)?;
        set_enabled(&self.send.get().expect("send"), !busy || self.pending.get().is_some())?;
        Ok(())
    }
    /// `private void send()`.
    pub fn send(self: &Arc<Self>) -> AppResult<()> {
        let input = self.input.get().expect("input");
        if self.pending.get().is_none() {
            let raw = ui::text_of(&input)?;
            let value = raw.trim_matches(|c: char| c <= ' ');
            if value.is_empty() {
                return Ok(());
            }
            let app = self.app();
            let pending = obj(&[
                ("request", JV::from(notemaker_core::ids::random_uuid())),
                ("text", JV::from(value)),
                ("document", JV::from(app.document.get())),
                ("session", JV::from(app.app_session_id.get())),
            ]);
            let editor = ui::edit(&app.this)?;
            ui::put_string(&editor, &self.pending_key(), &pending.to_string())?;
            ui::apply(&editor)?;
            self.pending.set(Some(pending));
        }
        let message = self.pending.get().unwrap();
        let key = self.pending_key();
        let draft = self.draft_key();
        let url = format!("{}/api/chats/{}/messages", s(&self.base.get()), s(&self.chat.get()));
        let token = self.generation.get();
        self.set_status("running", "Sending…")?;
        set_enabled(&self.send.get().expect("send"), false)?;
        set_enabled(&input, false)?;
        let me = self.clone();
        self.request(
            Box::new(move || {
                let result = me.http(&url, Some(&message)).and_then(|result| {
                    let editor = ui::edit(&me.context())?;
                    ui::remove_pref(&editor, &key)?;
                    ui::remove_pref(&editor, &draft)?;
                    ui::apply(&editor)?;
                    Ok(result)
                });
                let ui_me = me.clone();
                me.post(move || {
                    if token == ui_me.generation.get() {
                        if let Some(input) = ui_me.input.get() {
                            set_enabled(&input, true)?;
                        }
                    }
                    Ok(())
                });
                result
            }),
            Box::new(|me, _| {
                me.pending.set(None);
                ui::set_text(&me.input.get().expect("input"), "")?;
                let send = me.send.get().expect("send");
                ui::set_text(&send, "Send")?;
                set_enabled(&send, !me.busy.get())?;
                Ok(())
            }),
        );
        Ok(())
    }
    /// `private void stop()`.
    pub fn stop(self: &Arc<Self>) -> AppResult<()> {
        let url = format!("{}/api/chats/{}/stop", s(&self.base.get()), s(&self.chat.get()));
        let me = self.clone();
        self.request(
            Box::new(move || me.http(&url, Some(&JSONObject::new()))),
            Box::new(|me, result| {
                ui::set_text(&me.status_view(), if result.opt_boolean("stopping") { "Stopping…" } else { "Finished" })?;
                Ok(())
            }),
        );
        Ok(())
    }
    /// One connection of the reader loop; `Ok` when the stream ended or the
    /// generation changed.
    fn read_stream(self: &Arc<Self>, token: i32, host: &str, identity: &str, after: &mut i64, connection: &mut Option<Disconnector>) -> AppResult<()> {
        let mut c = HttpConnection::new(&format!("{host}/api/chats/{identity}/stream?after={after}"))?;
        c.set_request_property("Authorization", &format!("Bearer {}", s(&self.proxy_token.get())));
        c.set_connect_timeout(5000);
        c.set_read_timeout(10000);
        *connection = Some(c.disconnector());
        self.stream.set(Some(c.disconnector()));
        let mut lines = BufReader::new(c.input_stream()?);
        let me = self.clone();
        self.post(move || {
            if token == me.generation.get() && me.dialog.get().is_some() {
                ui::set_text(&me.status_view(), &me.last_status.get())?;
            }
            Ok(())
        });
        let mut raw = Vec::new();
        while token == self.generation.get() {
            raw.clear();
            if lines.read_until(b'\n', &mut raw)? == 0 {
                break;
            }
            let line = String::from_utf8_lossy(&raw);
            let line = line.strip_suffix('\n').unwrap_or(&line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let event = JSONObject::parse(line)?;
            if !event.has("seq") {
                continue;
            }
            let seq = event.get_long("seq")?;
            if seq <= *after {
                continue;
            }
            *after = seq;
            {
                let _g = self.incoming_lock.enter();
                if token != self.generation.get() {
                    break;
                }
                let next = event.get_json_object("event")?;
                self.incoming.with_mut(|i| i.push(next));
            }
            let me = self.clone();
            self.post(move || {
                if token == me.generation.get() && !me.rendering.get() {
                    me.rendering.set(true);
                    let flusher = me.clone();
                    let r = jx::try_once_runnable(move || flusher.flush(token).map_err(Into::into));
                    ui::post_delayed(&me.ui, &r, 250)?;
                }
                Ok(())
            });
        }
        Ok(())
    }
    /// `private void subscribe(int token, String host, String identity)`:
    /// starts the stream reader thread.
    pub fn subscribe(self: &Arc<Self>, token: i32, host: &str, identity: &str) {
        let me = self.clone();
        let (host, identity) = (host.to_string(), identity.to_string());
        JThread::spawn("agent-chat-stream", move || {
            let mut after: i64 = 0;
            while token == me.generation.get() {
                let mut connection: Option<Disconnector> = None;
                let stop = jx::frame(|| {
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| me.read_stream(token, &host, &identity, &mut after, &mut connection))).unwrap_or_else(|p| Err(panic_error(p)));
                    let mut stop = false;
                    if result.is_err() {
                        let ui_me = me.clone();
                        me.post(move || {
                            if token == ui_me.generation.get() && ui_me.dialog.get().is_some() {
                                ui::set_text(&ui_me.status_view(), "Disconnected · reconnecting. Draft kept on device.")?;
                            }
                            Ok(())
                        });
                        stop = JThread::sleep(1500).is_err();
                    }
                    stop
                });
                // finally { if (c != null) c.disconnect(); }
                if let Some(c) = connection {
                    c.disconnect();
                }
                if stop {
                    return;
                }
            }
        });
    }
    /// `private void flush(int token)` (UI thread).
    pub fn flush(self: &Arc<Self>, token: i32) -> AppResult<()> {
        self.rendering.set(false);
        if token != self.generation.get() || self.dialog.get().is_none() {
            return Ok(());
        }
        let events: Vec<JSONObject> = {
            let _g = self.incoming_lock.enter();
            self.incoming.take()
        };
        let scroll = self.scroll.get().expect("scroll");
        let messages = self.messages.get().expect("messages");
        let bottom = jcall!(scroll, "android/view/ViewGroup", "getChildCount", "()I")?.int() == 0
            || jcall!(scroll, "android/view/View", "getScrollY", "()I")?.int() + ui::height(&scroll)? >= ui::height(&messages)? - self.dp(60)?;
        let mut changed: Vec<Arc<Item>> = Vec::new();
        for event in events {
            let kind = event.opt_string("kind");
            let id = event.opt_string("id");
            if kind == "status" {
                self.set_status(&event.opt_string("status"), &event.opt_string("text"))?;
                continue;
            }
            if kind == "error" {
                ui::add_view(&messages, &self.text(&event.opt_string("text"), 14)?)?;
                continue;
            }
            if kind == "user" && self.pending.get().is_some_and(|p| id == p.opt_string("request")) {
                self.pending.set(None);
                ui::set_text(&self.input.get().expect("input"), "")?;
                ui::set_text(&self.send.get().expect("send"), "Send")?;
                let editor = ui::edit(&self.context())?;
                ui::remove_pref(&editor, &self.pending_key())?;
                ui::remove_pref(&editor, &self.draft_key())?;
                ui::apply(&editor)?;
            }
            let mut item = self.items.with(|i| i.get(&id).cloned());
            if item.is_none() && (kind == "user" || kind == "assistant" || kind == "tool") {
                let created = Item::new(self, &kind, &event)?;
                self.items.with_mut(|i| i.insert(id.clone(), created.clone()));
                ui::add_view(&messages, &created.view)?;
                item = Some(created);
            }
            let Some(item) = item else { continue };
            if kind == "delta" {
                item.value.with_mut(|v| v.push_str(&event.opt_string("text")));
            } else {
                item.value.set(event.opt_string("text"));
            }
            if event.has("status") {
                item.state.set(event.opt_string("status"));
            }
            if !changed.iter().any(|c| Arc::ptr_eq(c, &item)) {
                changed.push(item);
            }
        }
        for item in changed {
            item.refresh()?;
        }
        if bottom {
            let target = scroll.clone();
            let r = jx::try_once_runnable(move || {
                jcall!(target, "android/widget/ScrollView", "fullScroll", "(I)Z", VIEW_FOCUS_DOWN)?;
                Ok(())
            });
            ui::view_post(&scroll, &r)?;
        }
        Ok(())
    }
}

/// `private class Item`: one message row. Owned by `AgentUi.items`; the
/// heading's click listener captures it weakly.
pub struct Item {
    pub owner: Weak<AgentUi>,
    /// `final LinearLayout view = column()`.
    pub view: Obj,
    pub heading: Obj,
    pub detail: Obj,
    /// `final StringBuilder value`.
    pub value: Field<String>,
    pub kind: String,
    pub summary: String,
    pub arguments: String,
    pub state: Field<String>,
    pub expanded: Field<bool>,
}
impl Item {
    /// `Item(String kind, JSONObject event)`.
    pub fn new(owner: &Arc<AgentUi>, kind: &str, event: &JSONObject) -> AppResult<Arc<Item>> {
        let view = owner.column()?;
        let tool = ToolDisplay::new(&event.opt_string("name"), &event.opt_string("args"));
        let heading = owner.text(if kind == "user" { "You" } else { "Agent" }, 14)?;
        let detail = owner.text("", 16)?;
        jcall!(detail, "android/widget/TextView", "setTextIsSelectable", "(Z)V", true)?;
        ui::set_padding(&view, owner.dp(8)?, owner.dp(6)?, owner.dp(8)?, owner.dp(10)?)?;
        ui::add_view(&view, &heading)?;
        ui::add_view(&view, &detail)?;
        let item = Arc::new(Item {
            owner: Arc::downgrade(owner),
            view,
            heading: heading.clone(),
            detail: detail.clone(),
            value: Field::new(String::new()),
            kind: kind.to_string(),
            summary: tool.summary,
            arguments: tool.details,
            state: Field::new(String::new()),
            expanded: Field::new(false),
        });
        if kind == "tool" {
            let monospace = jsget!("android/graphics/Typeface", "MONOSPACE", "Landroid/graphics/Typeface;")?.some();
            jcall!(detail, "android/widget/TextView", "setTypeface", "(Landroid/graphics/Typeface;)V", &monospace)?;
            jcall!(detail, "android/widget/TextView", "setTextSize", "(F)V", 13.0f32)?;
            ui::set_content_description(&heading, "Expand tool activity")?;
            jcall!(heading, "android/widget/TextView", "setMinHeight", "(I)V", owner.dp(48)?)?;
            let weak = Arc::downgrade(&item);
            ui::on_click(&heading, move |_| {
                if let Some(item) = weak.upgrade() {
                    item.expanded.set(!item.expanded.get());
                    item.refresh()?;
                }
                Ok(())
            })?;
        }
        Ok(item)
    }
    /// `void refresh()`.
    pub fn refresh(&self) -> AppResult<()> {
        let value = self.value.get();
        if self.kind == "tool" {
            let expanded = self.expanded.get();
            ui::set_text(&self.heading, &format!("{}{} · {}", if expanded { "▾ " } else { "▸ " }, self.summary, self.state.get()))?;
            ui::set_visibility(&self.detail, if expanded { VIEW_VISIBLE } else { VIEW_GONE })?;
            if expanded {
                ui::set_text(&self.detail, &format!("{}{}", self.arguments, if value.is_empty() { String::new() } else { format!("\n\nOutput\n{value}") }))?;
            }
        } else if value != ui::text_of(&self.detail)? {
            ui::set_text(&self.detail, &value)?;
        }
        Ok(())
    }
}
