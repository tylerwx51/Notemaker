//! Port of `DocumentBrowser.java`: the Files dialog (grid/list of documents
//! with thumbnails). UI methods run on the main thread; listing, delete and
//! create run on `ReplicaService.writes`; thumbnails render on the private
//! `previews` executor.

use crate::android_canvas::{argb_8888, AndroidCanvas};
use crate::android_consts::{
    COLOR_BLACK, COLOR_LTGRAY, COLOR_WHITE, DIALOGINTERFACE_BUTTON_NEGATIVE, DIALOGINTERFACE_BUTTON_NEUTRAL, DIALOGINTERFACE_BUTTON_POSITIVE, GRAVITY_CENTER, GRAVITY_CENTER_VERTICAL, GRIDVIEW_AUTO_FIT, GRIDVIEW_STRETCH_COLUMN_WIDTH,
    LAYOUT_PARAMS_MATCH_PARENT, LINEARLAYOUT_HORIZONTAL, R_LAYOUT_SIMPLE_SPINNER_DROPDOWN_ITEM, TOAST_LENGTH_LONG,
};
use crate::app_replica::AppReplica;
use crate::concurrent::ExecutorService;
use crate::error::{AppError, AppResult};
use crate::field::Field;
use crate::jx::{self, JResult, Obj, Reply};
use crate::main_activity::MainActivity;
use crate::replica_service;
use crate::ui;
use notemaker_cli::png::Png;
use notemaker_core::draw_item::DrawItem;
use notemaker_core::json::{self, Map, Value};
use notemaker_core::{jmath, object, render};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::{Arc, Weak};

/// `final class DocumentBrowser`. The anonymous `BaseAdapter`
/// (`DocumentBrowser$1`) is a `jx::adapter` closure and the search
/// `TextWatcher` (`DocumentBrowser$2`) a `jx::callback`; both capture the
/// browser weakly.
pub struct DocumentBrowser {
    pub app: Weak<MainActivity>,
    /// `final AppReplica source = app.replica`.
    pub source: Arc<AppReplica>,
    pub names: Field<Vec<String>>,
    pub visible: Field<Vec<String>>,
    pub dialog: Field<Option<Obj>>,
    pub adapter: Field<Option<Obj>>,
    pub files: Field<Option<Obj>>,
    pub grid_mode: Field<bool>,
    /// `volatile boolean closed`.
    pub closed: Field<bool>,
    /// `volatile int generation`.
    pub generation: Field<i32>,
    /// `Executors.newSingleThreadExecutor()`.
    pub previews: ExecutorService,
    /// `android.util.LruCache<String,Bitmap>(24)` (the Java object).
    pub thumbnails: Obj,
    pub loading: Field<HashSet<String>>,
    pub failed: Field<HashSet<String>>,
    pub status: Field<Option<Obj>>,
    pub search: Field<Option<Obj>>,
    pub mode_group: Field<Option<Obj>>,
    pub grid_button_id: Field<i32>,
    pub list_button_id: Field<i32>,
    pub loaded: Field<bool>,
}

fn text_view_with(context: &Obj, value: &str) -> JResult<Obj> {
    let v = ui::text_view(context)?;
    ui::set_text(&v, value)?;
    Ok(v)
}
fn set_hint(view: &Obj, hint: &str) -> JResult<()> {
    let s = jx::jstr(hint);
    jcall!(view, "android/widget/TextView", "setHint", "(Ljava/lang/CharSequence;)V", &s)?;
    Ok(())
}
fn set_enabled(view: &Obj, enabled: bool) -> JResult<()> {
    jcall!(view, "android/view/View", "setEnabled", "(Z)V", enabled)?;
    Ok(())
}
fn button(dialog: &Obj, which: i32) -> JResult<Obj> {
    Ok(jcall!(dialog, "android/app/AlertDialog", "getButton", "(I)Landroid/widget/Button;", which)?.some())
}
fn dismiss_dialog(dialog: &Obj) -> JResult<()> {
    jcall!(dialog, "android/app/Dialog", "dismiss", "()V")?;
    Ok(())
}
fn is_showing(dialog: &Obj) -> JResult<bool> {
    Ok(jcall!(dialog, "android/app/Dialog", "isShowing", "()Z")?.bool())
}
fn set_cancelable(dialog: &Obj, value: bool) -> JResult<()> {
    jcall!(dialog, "android/app/Dialog", "setCancelable", "(Z)V", value)?;
    Ok(())
}
fn builder(context: &Obj, title: &str) -> JResult<Obj> {
    let b = jnew!("android/app/AlertDialog$Builder", "(Landroid/content/Context;)V", context)?;
    let t = jx::jstr(title);
    jcall!(b, "android/app/AlertDialog$Builder", "setTitle", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", &t)?;
    Ok(b)
}
fn set_view(builder: &Obj, view: &Obj) -> JResult<()> {
    jcall!(builder, "android/app/AlertDialog$Builder", "setView", "(Landroid/view/View;)Landroid/app/AlertDialog$Builder;", view)?;
    Ok(())
}
fn positive(builder: &Obj, label: &str) -> JResult<()> {
    let s = jx::jstr(label);
    jcall!(builder, "android/app/AlertDialog$Builder", "setPositiveButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", &s, jx::NULL)?;
    Ok(())
}
fn negative(builder: &Obj, label: &str) -> JResult<()> {
    let s = jx::jstr(label);
    jcall!(builder, "android/app/AlertDialog$Builder", "setNegativeButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", &s, jx::NULL)?;
    Ok(())
}
fn neutral(builder: &Obj, label: &str) -> JResult<()> {
    let s = jx::jstr(label);
    jcall!(builder, "android/app/AlertDialog$Builder", "setNeutralButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", &s, jx::NULL)?;
    Ok(())
}
fn create_dialog(builder: &Obj) -> JResult<Obj> {
    Ok(jcall!(builder, "android/app/AlertDialog$Builder", "create", "()Landroid/app/AlertDialog;")?.some())
}
fn on_show(dialog: &Obj, f: impl Fn() -> AppResult<()> + Send + Sync + 'static) -> JResult<()> {
    let listener = jx::callback(move |_, _| f().map(|_| Reply::Void).map_err(Into::into));
    jcall!(dialog, "android/app/Dialog", "setOnShowListener", "(Landroid/content/DialogInterface$OnShowListener;)V", &listener)?;
    Ok(())
}
fn show_dialog(dialog: &Obj) -> JResult<()> {
    jcall!(dialog, "android/app/Dialog", "show", "()V")?;
    Ok(())
}
fn notify_changed(adapter: &Obj) -> JResult<()> {
    jcall!(adapter, "android/widget/BaseAdapter", "notifyDataSetChanged", "()V")?;
    Ok(())
}
fn panic_message(p: Box<dyn std::any::Any + Send>) -> AppError {
    let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into());
    AppError::runtime(msg)
}
/// Java `String.trim()`.
fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}
fn upper_unit(u: u16) -> u16 {
    match char::from_u32(u as u32) {
        Some(c) => {
            let mut it = c.to_uppercase();
            match (it.next(), it.next()) {
                (Some(x), None) if (x as u32) <= 0xffff => x as u32 as u16,
                _ => u,
            }
        }
        None => u,
    }
}
fn lower_unit(u: u16) -> u16 {
    match char::from_u32(u as u32) {
        Some(c) => {
            let mut it = c.to_lowercase();
            match (it.next(), it.next()) {
                (Some(x), None) if (x as u32) <= 0xffff => x as u32 as u16,
                _ => u,
            }
        }
        None => u,
    }
}
/// `String.CASE_INSENSITIVE_ORDER.thenComparing(Comparator.naturalOrder())`.
fn case_insensitive_then_natural(a: &str, b: &str) -> Ordering {
    let x: Vec<u16> = a.encode_utf16().collect();
    let y: Vec<u16> = b.encode_utf16().collect();
    for (&c1, &c2) in x.iter().zip(&y) {
        if c1 != c2 {
            let (u1, u2) = (upper_unit(c1), upper_unit(c2));
            if u1 != u2 {
                let (l1, l2) = (lower_unit(u1), lower_unit(u2));
                if l1 != l2 {
                    return l1.cmp(&l2);
                }
            }
        }
    }
    x.len().cmp(&y.len()).then_with(|| x.cmp(&y))
}

impl DocumentBrowser {
    /// `DocumentBrowser(MainActivity app)`.
    pub fn new(app: &Arc<MainActivity>) -> AppResult<Arc<DocumentBrowser>> {
        let thumbnails = jnew!("android/util/LruCache", "(I)V", 24)?;
        Ok(Arc::new(DocumentBrowser {
            app: Arc::downgrade(app),
            source: app.replica(),
            names: Field::new(Vec::new()),
            visible: Field::new(Vec::new()),
            dialog: Field::new(None),
            adapter: Field::new(None),
            files: Field::new(None),
            grid_mode: Field::new(false),
            closed: Field::new(false),
            generation: Field::new(0),
            previews: ExecutorService::single_thread(),
            thumbnails,
            loading: Field::new(HashSet::new()),
            failed: Field::new(HashSet::new()),
            status: Field::new(None),
            search: Field::new(None),
            mode_group: Field::new(None),
            grid_button_id: Field::new(0),
            list_button_id: Field::new(0),
            loaded: Field::new(false),
        }))
    }
    pub fn app(&self) -> Arc<MainActivity> {
        self.app.upgrade().expect("MainActivity dropped")
    }
    fn dialog_obj(&self) -> Obj {
        self.dialog.get().expect("dialog")
    }
    /// `void show()`: also sets `app.browser = this`.
    pub fn show(self: &Arc<Self>) -> AppResult<()> {
        let app = self.app();
        app.browser.set(Some(self.clone()));
        let ctx = app.this.clone();
        let editor = app.editor();
        let body = ui::column(&ctx)?;
        let padding = editor.dp(20.0)?;
        ui::set_padding(&body, padding, 0, padding, 0)?;
        let scope = text_view_with(&ctx, if self.source.remote() { "Files on the selected editing proxy" } else { "Files on this device · available offline" })?;
        ui::add_view(&body, &scope)?;
        let search = jnew!("android/widget/EditText", "(Landroid/content/Context;)V", &ctx)?;
        jcall!(search, "android/widget/TextView", "setSingleLine", "()V")?;
        set_hint(&search, "Search files")?;
        ui::add_view(&body, &search)?;
        self.search.set(Some(search.clone()));
        self.grid_mode.set(ui::pref_bool(&ctx, "filesGrid", true)?);
        let modes = jnew!("android/widget/RadioGroup", "(Landroid/content/Context;)V", &ctx)?;
        jcall!(modes, "android/widget/LinearLayout", "setOrientation", "(I)V", LINEARLAYOUT_HORIZONTAL)?;
        let grid = jnew!("android/widget/RadioButton", "(Landroid/content/Context;)V", &ctx)?;
        jcall!(grid, "android/view/View", "setId", "(I)V", jstatic!("android/view/View", "generateViewId", "()I")?.int())?;
        ui::set_text(&grid, "Grid")?;
        ui::add_view(&modes, &grid)?;
        let list = jnew!("android/widget/RadioButton", "(Landroid/content/Context;)V", &ctx)?;
        jcall!(list, "android/view/View", "setId", "(I)V", jstatic!("android/view/View", "generateViewId", "()I")?.int())?;
        ui::set_text(&list, "List")?;
        ui::add_view(&modes, &list)?;
        let grid_id = jcall!(grid, "android/view/View", "getId", "()I")?.int();
        let list_id = jcall!(list, "android/view/View", "getId", "()I")?.int();
        self.mode_group.set(Some(modes.clone()));
        self.grid_button_id.set(grid_id);
        self.list_button_id.set(list_id);
        jcall!(modes, "android/widget/RadioGroup", "check", "(I)V", if self.grid_mode.get() { grid_id } else { list_id })?;
        ui::add_view(&body, &modes)?;
        let status = ui::text_view(&ctx)?;
        ui::add_view(&body, &status)?;
        self.status.set(Some(status));
        let files = jnew!("android/widget/GridView", "(Landroid/content/Context;)V", &ctx)?;
        jcall!(files, "android/widget/GridView", "setStretchMode", "(I)V", GRIDVIEW_STRETCH_COLUMN_WIDTH)?;
        jcall!(files, "android/widget/GridView", "setColumnWidth", "(I)V", editor.dp(150.0)?)?;
        jcall!(files, "android/widget/GridView", "setHorizontalSpacing", "(I)V", editor.dp(12.0)?)?;
        jcall!(files, "android/widget/GridView", "setVerticalSpacing", "(I)V", editor.dp(12.0)?)?;
        self.files.set(Some(files.clone()));
        let weak = Arc::downgrade(self);
        let adapter = jx::adapter(move |method, args| {
            let Some(me) = weak.upgrade() else { return Ok(Reply::Void) };
            match method {
                "getCount()" => Ok(Reply::Int(me.visible.with(|v| v.len()) as i32)),
                "getItem(int)" => {
                    let position = jx::unbox_int(&args[0]) as usize;
                    Ok(Reply::Obj(Some(jx::jstr(&me.visible.with(|v| v[position].clone())))))
                }
                "getItemId(int)" => Ok(Reply::Void),
                "getView(int,View,ViewGroup)" => {
                    let position = jx::unbox_int(&args[0]) as usize;
                    let name = me.visible.with(|v| v[position].clone());
                    Ok(Reply::Obj(Some(me.file_view(&name)?)))
                }
                _ => Ok(Reply::Void),
            }
        })?;
        self.adapter.set(Some(adapter.clone()));
        jcall!(files, "android/widget/GridView", "setAdapter", "(Landroid/widget/ListAdapter;)V", &adapter)?;
        self.display_mode()?;
        let weak = Arc::downgrade(self);
        let checked = jx::callback(move |_, args| {
            let Some(me) = weak.upgrade() else { return Ok(Reply::Void) };
            let id = jx::unbox_int(&args[1]);
            me.grid_mode.set(id == grid_id);
            let app = me.app();
            let editor = ui::edit(&app.this)?;
            ui::put_bool(&editor, "filesGrid", me.grid_mode.get())?;
            ui::apply(&editor)?;
            me.display_mode()?;
            Ok(Reply::Void)
        });
        jcall!(modes, "android/widget/RadioGroup", "setOnCheckedChangeListener", "(Landroid/widget/RadioGroup$OnCheckedChangeListener;)V", &checked)?;
        let params = ui::linear_params(LAYOUT_PARAMS_MATCH_PARENT, editor.dp(360.0)?)?;
        ui::add_view_params(&body, &files, &params)?;
        let weak = Arc::downgrade(self);
        let click = jx::callback(move |_, args| {
            let Some(me) = weak.upgrade() else { return Ok(Reply::Void) };
            let position = jx::unbox_int(&args[2]) as usize;
            let name = me.visible.with(|v| v[position].clone());
            dismiss_dialog(&me.dialog_obj())?;
            me.app().open_document(&name, "fixed")?;
            Ok(Reply::Void)
        });
        jcall!(files, "android/widget/AdapterView", "setOnItemClickListener", "(Landroid/widget/AdapterView$OnItemClickListener;)V", &click)?;
        let weak = Arc::downgrade(self);
        let long_click = jx::callback(move |_, args| {
            let Some(me) = weak.upgrade() else { return Ok(Reply::Bool(true)) };
            let position = jx::unbox_int(&args[2]) as usize;
            let name = me.visible.with(|v| v[position].clone());
            me.delete(&name)?;
            Ok(Reply::Bool(true))
        });
        jcall!(files, "android/widget/AdapterView", "setOnItemLongClickListener", "(Landroid/widget/AdapterView$OnItemLongClickListener;)V", &long_click)?;
        let weak = Arc::downgrade(self);
        let watcher = jx::callback(move |method, _| {
            if method == "onTextChanged(CharSequence,int,int,int)" {
                if let Some(me) = weak.upgrade() {
                    me.filter()?;
                }
            }
            Ok(Reply::Void)
        });
        jcall!(search, "android/widget/TextView", "addTextChangedListener", "(Landroid/text/TextWatcher;)V", &watcher)?;
        let b = builder(&ctx, "Files")?;
        set_view(&b, &body)?;
        positive(&b, "New file")?;
        neutral(&b, "Refresh")?;
        negative(&b, "Close")?;
        let dialog = create_dialog(&b)?;
        self.dialog.set(Some(dialog.clone()));
        let weak = Arc::downgrade(self);
        let dismissed = jx::callback(move |_, _| {
            let Some(me) = weak.upgrade() else { return Ok(Reply::Void) };
            me.closed.set(true);
            let app = me.app();
            if app.browser.get().is_some_and(|b| Arc::ptr_eq(&b, &me)) {
                app.browser.set(None);
            }
            me.previews.shutdown_now();
            jcall!(me.thumbnails, "android/util/LruCache", "evictAll", "()V")?;
            Ok(Reply::Void)
        });
        jcall!(dialog, "android/app/Dialog", "setOnDismissListener", "(Landroid/content/DialogInterface$OnDismissListener;)V", &dismissed)?;
        let weak = Arc::downgrade(self);
        on_show(&dialog, move || {
            let Some(me) = weak.upgrade() else { return Ok(()) };
            let dialog = me.dialog_obj();
            let w = Arc::downgrade(&me);
            ui::on_click(&button(&dialog, DIALOGINTERFACE_BUTTON_POSITIVE)?, move |_| match w.upgrade() {
                Some(me) => me.create().map_err(Into::into),
                None => Ok(()),
            })?;
            let w = Arc::downgrade(&me);
            ui::on_click(&button(&dialog, DIALOGINTERFACE_BUTTON_NEUTRAL)?, move |_| match w.upgrade() {
                Some(me) => me.refresh().map_err(Into::into),
                None => Ok(()),
            })?;
            me.refresh()
        })?;
        show_dialog(&dialog)?;
        Ok(())
    }
    /// `void dismiss()`.
    pub fn dismiss(&self) -> AppResult<()> {
        if let Some(dialog) = self.dialog.get() {
            dismiss_dialog(&dialog)?;
        }
        Ok(())
    }
    /// `boolean showing()`.
    pub fn showing(&self) -> AppResult<bool> {
        Ok(match self.dialog.get() {
            Some(dialog) => is_showing(&dialog)?,
            None => false,
        })
    }
    /// `void search(String query)`.
    pub fn search(self: &Arc<Self>, query: &str) -> AppResult<()> {
        let search = self.search.get().ok_or_else(|| AppError::java("java.lang.NullPointerException", "search"))?;
        ui::set_text(&search, query)?;
        Ok(())
    }
    /// `void mode(String value)`.
    pub fn mode(self: &Arc<Self>, value: &str) -> AppResult<()> {
        if value != "grid" && value != "list" {
            return Err(AppError::illegal_argument("Mode must be grid or list"));
        }
        let group = self.mode_group.get().ok_or_else(|| AppError::java("java.lang.NullPointerException", "modeGroup"))?;
        jcall!(group, "android/widget/RadioGroup", "check", "(I)V", if value == "grid" { self.grid_button_id.get() } else { self.list_button_id.get() })?;
        Ok(())
    }
    /// `void refreshNow()`.
    pub fn refresh_now(self: &Arc<Self>) -> AppResult<()> {
        self.refresh()
    }
    /// `Map<String,Object> state()`.
    pub fn state(&self) -> AppResult<Map> {
        let search = self.search.get().ok_or_else(|| AppError::java("java.lang.NullPointerException", "search"))?;
        let visible: Vec<Value> = self.visible.with(|v| v.iter().map(|s| Value::from(s.as_str())).collect());
        Ok(object! {
            "mode" => if self.grid_mode.get() { "grid" } else { "list" },
            "query" => ui::text_of(&search)?,
            "loaded" => self.loaded.get(),
            "visible" => visible,
        })
    }
    /// `private boolean current()`.
    pub fn current(&self) -> bool {
        let app = self.app();
        let showing = self.dialog.get().map(|d| is_showing(&d).unwrap_or(false)).unwrap_or(false);
        let finishing = jcall!(app.this, "android/app/Activity", "isFinishing", "()Z").map(|r| r.bool()).unwrap_or(false);
        showing && !finishing && app.replica.get().is_some_and(|r| Arc::ptr_eq(&r, &self.source))
    }
    /// `private void displayMode()`.
    pub fn display_mode(self: &Arc<Self>) -> AppResult<()> {
        let files = self.files.get().expect("files");
        let grid = self.grid_mode.get();
        jcall!(files, "android/widget/GridView", "setNumColumns", "(I)V", if grid { GRIDVIEW_AUTO_FIT } else { 1 })?;
        ui::set_content_description(&files, if grid { "Files grid" } else { "Files list" })?;
        notify_changed(&self.adapter.get().expect("adapter"))?;
        Ok(())
    }
    /// `private View fileView(String name)` (adapter `getView`).
    pub fn file_view(self: &Arc<Self>, name: &str) -> AppResult<Obj> {
        let app = self.app();
        let ctx = app.this.clone();
        let editor = app.editor();
        let tile = ui::column(&ctx)?;
        let pad = editor.dp(8.0)?;
        ui::set_padding(&tile, pad, pad, pad, pad)?;
        let border = jnew!("android/graphics/drawable/GradientDrawable", "()V")?;
        jcall!(border, "android/graphics/drawable/GradientDrawable", "setColor", "(I)V", COLOR_WHITE)?;
        jcall!(border, "android/graphics/drawable/GradientDrawable", "setStroke", "(II)V", editor.dp(1.0)?, COLOR_LTGRAY)?;
        jcall!(tile, "android/view/View", "setBackground", "(Landroid/graphics/drawable/Drawable;)V", &border)?;
        if self.grid_mode.get() {
            let key = jx::jstr(name);
            let bitmap = jcall!(self.thumbnails, "android/util/LruCache", "get", "(Ljava/lang/Object;)Ljava/lang/Object;", &key)?.global();
            if let Some(bitmap) = bitmap {
                let image = jnew!("android/widget/ImageView", "(Landroid/content/Context;)V", &ctx)?;
                jcall!(image, "android/widget/ImageView", "setImageBitmap", "(Landroid/graphics/Bitmap;)V", &bitmap)?;
                let fit = jsget!("android/widget/ImageView$ScaleType", "FIT_CENTER", "Landroid/widget/ImageView$ScaleType;")?.some();
                jcall!(image, "android/widget/ImageView", "setScaleType", "(Landroid/widget/ImageView$ScaleType;)V", &fit)?;
                ui::set_content_description(&image, &format!("Preview of {name}"))?;
                ui::add_view_params(&tile, &image, &ui::linear_params(-1, editor.dp(160.0)?)?)?;
            } else {
                let failed = self.failed.with(|f| f.contains(name));
                let placeholder = ui::text_view(&ctx)?;
                jcall!(placeholder, "android/widget/TextView", "setGravity", "(I)V", GRAVITY_CENTER)?;
                ui::set_text(&placeholder, if failed { "Preview unavailable" } else { "Loading preview…" })?;
                ui::add_view_params(&tile, &placeholder, &ui::linear_params(-1, editor.dp(160.0)?)?)?;
                if !failed {
                    self.preview(name);
                }
            }
        }
        let label = text_view_with(&ctx, name)?;
        jcall!(label, "android/widget/TextView", "setTextColor", "(I)V", COLOR_BLACK)?;
        jcall!(label, "android/widget/TextView", "setTextSize", "(F)V", 16.0f32)?;
        jcall!(label, "android/widget/TextView", "setMaxLines", "(I)V", 2)?;
        let end = jsget!("android/text/TextUtils$TruncateAt", "END", "Landroid/text/TextUtils$TruncateAt;")?.some();
        jcall!(label, "android/widget/TextView", "setEllipsize", "(Landroid/text/TextUtils$TruncateAt;)V", &end)?;
        jcall!(label, "android/widget/TextView", "setMinHeight", "(I)V", editor.dp(48.0)?)?;
        jcall!(label, "android/widget/TextView", "setGravity", "(I)V", GRAVITY_CENTER_VERTICAL)?;
        ui::add_view(&tile, &label)?;
        Ok(tile)
    }
    /// `private void preview(String name)`: queues a thumbnail on `previews`.
    pub fn preview(self: &Arc<Self>, name: &str) {
        if self.closed.get() || !self.loading.with_mut(|l| l.insert(name.to_string())) {
            return;
        }
        let request = self.generation.get();
        let me = self.clone();
        let app = self.app();
        let name = name.to_string();
        self.previews.execute(move || {
            if me.closed.get() || request != me.generation.get() {
                return;
            }
            let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> AppResult<Obj> {
                let snapshot = me.source.command_named(&name, &["snapshot"], None)?;
                DocumentBrowser::thumbnail(json::map(&snapshot)?)
            }))
            .unwrap_or_else(|p| Err(panic_message(p)));
            let bitmap = match rendered {
                Ok(b) => Some(b),
                Err(e) => {
                    jx::log::w_err("NoteMaker", "File preview unavailable", &e);
                    None
                }
            };
            let me2 = me.clone();
            app.run_on_ui_thread(move |_| {
                let me = me2;
                if me.closed.get() || request != me.generation.get() {
                    return Ok(());
                }
                me.loading.with_mut(|l| l.remove(&name));
                match bitmap {
                    None => {
                        me.failed.with_mut(|f| f.insert(name.clone()));
                    }
                    Some(bitmap) => {
                        let key = jx::jstr(&name);
                        jcall!(me.thumbnails, "android/util/LruCache", "put", "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;", &key, &bitmap)?;
                    }
                }
                if me.current() && me.grid_mode.get() {
                    notify_changed(&me.adapter.get().expect("adapter"))?;
                }
                Ok(())
            });
        });
    }
    /// `static Bitmap thumbnail(Map<String,Object> state)` (any thread).
    /// Like Java's `Store.model(state)`, an in-process snapshot's model is
    /// used as is.
    pub fn thumbnail<S: crate::main_activity::SharedState + ?Sized>(shared: &S) -> AppResult<Obj> {
        let model = shared.shared_model()?;
        let state: &Map = shared.map();
        let mut hidden: HashSet<String> = HashSet::new();
        let empty = Value::Array(Vec::new());
        for layer in json::list(state.get("hidden").unwrap_or(&empty))? {
            hidden.insert(json::java_string(Some(layer)));
        }
        let (mut left, mut top, mut right, mut bottom) = (0.0f64, 0.0f64, 1000.0f64, 1400.0f64);
        if model.type_ == "infinite" {
            let mut found = false;
            for (layer, value) in &model.layers {
                if model.layer_active.get(layer) == Some(&false) || (hidden.contains(layer) && !json::is_true(value.get("force"))) {
                    continue;
                }
                for content in model.layer_items(layer, 0).iter() {
                    let item = DrawItem::new(&model, content.clone())?;
                    if !item.active() {
                        continue;
                    }
                    let b = item.paint_bounds(&notemaker_cli::item_coverage::FACTORY)?;
                    if !found {
                        left = b[0];
                        top = b[1];
                        right = b[2];
                        bottom = b[3];
                        found = true;
                    } else {
                        left = jmath::min(left, b[0]);
                        top = jmath::min(top, b[1]);
                        right = jmath::max(right, b[2]);
                        bottom = jmath::max(bottom, b[3]);
                    }
                }
            }
            if found {
                left -= 30.0;
                top -= 30.0;
                right += 30.0;
                bottom += 30.0;
            }
        }
        let bitmap = jstatic!("android/graphics/Bitmap", "createBitmap", "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;", 240, 336, &argb_8888()?)?.some();
        let canvas = jnew!("android/graphics/Canvas", "(Landroid/graphics/Bitmap;)V", &bitmap)?;
        jcall!(canvas, "android/graphics/Canvas", "drawColor", "(I)V", COLOR_WHITE)?;
        let scale = jmath::min(232.0 / jmath::max(1.0, right - left), 328.0 / jmath::max(1.0, bottom - top)) as f32;
        jcall!(canvas, "android/graphics/Canvas", "translate", "(FF)V", (240.0 - (right - left) as f32 * scale) / 2.0, (336.0 - (bottom - top) as f32 * scale) / 2.0)?;
        jcall!(canvas, "android/graphics/Canvas", "scale", "(FF)V", scale, scale)?;
        jcall!(canvas, "android/graphics/Canvas", "translate", "(FF)V", -left as f32, -top as f32)?;
        let mut target = AndroidCanvas::new(canvas);
        render::draw(&model, 0, &hidden, &mut Png::new(&mut target))?;
        Ok(bitmap)
    }
    /// `private void filter()`.
    pub fn filter(self: &Arc<Self>) -> AppResult<()> {
        let search = self.search.get().expect("search");
        let query = trim(&ui::text_of(&search)?).to_lowercase();
        let names = self.names.get();
        let visible: Vec<String> = names.iter().filter(|n| n.to_lowercase().contains(&query)).cloned().collect();
        let count = visible.len();
        self.visible.set(visible);
        notify_changed(&self.adapter.get().expect("adapter"))?;
        if self.loaded.get() {
            let text = if names.is_empty() {
                "No files yet. Choose New file to begin.".to_string()
            } else if count == 0 {
                "No matching files.".to_string()
            } else {
                format!("{count} files · tap to open · hold to delete")
            };
            ui::set_text(&self.status.get().expect("status"), &text)?;
        }
        Ok(())
    }
    /// `private void delete(String name)`.
    pub fn delete(self: &Arc<Self>, name: &str) -> AppResult<()> {
        let app = self.app();
        let b = builder(&app.this, "Delete file?")?;
        let message = jx::jstr(&format!("Delete “{name}” from this replica and the server when synchronized? This cannot be undone, and this name cannot be reused."));
        jcall!(b, "android/app/AlertDialog$Builder", "setMessage", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", &message)?;
        positive(&b, "Delete")?;
        negative(&b, "Cancel")?;
        let confirm = create_dialog(&b)?;
        let weak = Arc::downgrade(self);
        let name = name.to_string();
        let dialog = confirm.clone();
        on_show(&confirm, move || {
            let weak = weak.clone();
            let name = name.clone();
            let confirm = dialog.clone();
            ui::on_click(&button(&dialog, DIALOGINTERFACE_BUTTON_POSITIVE)?, move |_| {
                let Some(me) = weak.upgrade() else { return Ok(()) };
                set_enabled(&button(&confirm, DIALOGINTERFACE_BUTTON_POSITIVE)?, false)?;
                set_enabled(&button(&confirm, DIALOGINTERFACE_BUTTON_NEGATIVE)?, false)?;
                set_cancelable(&confirm, false)?;
                let name = name.clone();
                let confirm = confirm.clone();
                let app = me.app();
                replica_service::writes().execute(move || {
                    let work = || -> AppResult<Option<notemaker_core::store::State>> {
                        me.source.command_named(&name, &["document", "delete"], None)?;
                        Ok(if name == me.source.document { Some(me.source.read()?) } else { None })
                    };
                    match work() {
                        Ok(snapshot) => app.run_on_ui_thread(move |app| {
                            dismiss_dialog(&confirm)?;
                            if !me.current() {
                                return Ok(());
                            }
                            if let Some(snapshot) = snapshot {
                                if let Err(e) = app.refresh_shared(&snapshot).and_then(|_| app.update()) {
                                    jx::log::w_err("NoteMaker", "Deleted file refresh failed", &e);
                                }
                            }
                            me.refresh()
                        }),
                        Err(e) => app.run_on_ui_thread(move |app| {
                            dismiss_dialog(&confirm)?;
                            if !me.current() {
                                return Ok(());
                            }
                            ui::toast(&app.this, &format!("Could not delete file: {}", e.message_or_null()), TOAST_LENGTH_LONG)?;
                            Ok(())
                        }),
                    }
                });
                Ok(())
            })?;
            Ok(())
        })?;
        show_dialog(&confirm)?;
        Ok(())
    }
    /// `private void refresh()`.
    pub fn refresh(self: &Arc<Self>) -> AppResult<()> {
        self.generation.set(self.generation.get() + 1);
        jcall!(self.thumbnails, "android/util/LruCache", "evictAll", "()V")?;
        self.failed.with_mut(|f| f.clear());
        self.loading.with_mut(|l| l.clear());
        self.loaded.set(false);
        ui::set_text(&self.status.get().expect("status"), "Loading files…")?;
        set_enabled(&button(&self.dialog_obj(), DIALOGINTERFACE_BUTTON_NEUTRAL)?, false)?;
        let me = self.clone();
        let app = self.app();
        replica_service::writes().execute(move || {
            let work = || -> AppResult<Vec<String>> {
                let listed = me.source.command(&["document", "list"], None)?;
                let mut result: Vec<String> = Vec::new();
                for name in json::list(json::map(&listed)?.get("documents").unwrap_or(&Value::Null))? {
                    result.push(json::java_string(Some(name)));
                }
                result.sort_by(|a, b| case_insensitive_then_natural(a, b));
                Ok(result)
            };
            match work() {
                Ok(result) => app.run_on_ui_thread(move |_| {
                    if !me.current() {
                        return Ok(());
                    }
                    me.names.set(result);
                    me.loaded.set(true);
                    me.filter()?;
                    set_enabled(&button(&me.dialog_obj(), DIALOGINTERFACE_BUTTON_NEUTRAL)?, true)?;
                    Ok(())
                }),
                Err(_) => app.run_on_ui_thread(move |_| {
                    if !me.current() {
                        return Ok(());
                    }
                    ui::set_text(&me.status.get().expect("status"), "Could not load files. Check the editing proxy and tap Refresh.")?;
                    set_enabled(&button(&me.dialog_obj(), DIALOGINTERFACE_BUTTON_NEUTRAL)?, true)?;
                    Ok(())
                }),
            }
        });
        Ok(())
    }
    /// `private void create()`.
    pub fn create(self: &Arc<Self>) -> AppResult<()> {
        let app = self.app();
        let ctx = app.this.clone();
        let fields = ui::column(&ctx)?;
        let padding = app.editor().dp(20.0)?;
        ui::set_padding(&fields, padding, 0, padding, 0)?;
        let name = jnew!("android/widget/EditText", "(Landroid/content/Context;)V", &ctx)?;
        jcall!(name, "android/widget/TextView", "setSingleLine", "()V")?;
        set_hint(&name, "File name")?;
        ui::add_view(&fields, &name)?;
        let kind = jnew!("android/widget/Spinner", "(Landroid/content/Context;)V", &ctx)?;
        let choices = jx::new_string_array(&["Fixed page", "Multiple pages", "Infinite canvas"])?;
        let adapter = jnew!("android/widget/ArrayAdapter", "(Landroid/content/Context;I[Ljava/lang/Object;)V", &ctx, R_LAYOUT_SIMPLE_SPINNER_DROPDOWN_ITEM, &choices)?;
        jcall!(kind, "android/widget/Spinner", "setAdapter", "(Landroid/widget/SpinnerAdapter;)V", &adapter)?;
        ui::add_view(&fields, &kind)?;
        let error = ui::text_view(&ctx)?;
        ui::add_view(&fields, &error)?;
        let b = builder(&ctx, "New file")?;
        set_view(&b, &fields)?;
        positive(&b, "Create")?;
        negative(&b, "Cancel")?;
        let create = create_dialog(&b)?;
        let weak = Arc::downgrade(self);
        let dialog = create.clone();
        on_show(&create, move || {
            let weak = weak.clone();
            let (create, name, kind, error) = (dialog.clone(), name.clone(), kind.clone(), error.clone());
            ui::on_click(&button(&dialog, DIALOGINTERFACE_BUTTON_POSITIVE)?, move |_| {
                let Some(me) = weak.upgrade() else { return Ok(()) };
                let next = trim(&ui::text_of(&name)?).to_string();
                if next.is_empty() || json::jlen(&next) > 200 {
                    ui::set_text(&error, "Enter a file name between 1 and 200 characters")?;
                    return Ok(());
                }
                let position = jcall!(kind, "android/widget/AdapterView", "getSelectedItemPosition", "()I")?.int();
                let document_kind = ["fixed", "pages", "infinite"]
                    .get(position as usize)
                    .ok_or_else(|| jx::JavaError::new("java.lang.ArrayIndexOutOfBoundsException", format!("Index {position} out of bounds for length 3")))?
                    .to_string();
                set_enabled(&button(&create, DIALOGINTERFACE_BUTTON_POSITIVE)?, false)?;
                set_enabled(&button(&create, DIALOGINTERFACE_BUTTON_NEGATIVE)?, false)?;
                set_cancelable(&create, false)?;
                let (create, error) = (create.clone(), error.clone());
                let app = me.app();
                replica_service::writes().execute(move || {
                    let work = || -> AppResult<()> {
                        let target = AppReplica::new(&app, &next, &app.proxy_url.get(), &app.proxy_token.get())?;
                        target.command(&["document", "create", &document_kind], None)?;
                        Ok(())
                    };
                    match work() {
                        Ok(()) => app.run_on_ui_thread(move |app| {
                            dismiss_dialog(&create)?;
                            if !me.current() {
                                return Ok(());
                            }
                            dismiss_dialog(&me.dialog_obj())?;
                            app.open_document(&next, &document_kind)
                        }),
                        Err(e) => app.run_on_ui_thread(move |_| {
                            if !is_showing(&create)? {
                                return Ok(());
                            }
                            ui::set_text(&error, &format!("Could not create file: {}", e.message_or_null()))?;
                            set_enabled(&button(&create, DIALOGINTERFACE_BUTTON_POSITIVE)?, true)?;
                            set_enabled(&button(&create, DIALOGINTERFACE_BUTTON_NEGATIVE)?, true)?;
                            set_cancelable(&create, true)?;
                            Ok(())
                        }),
                    }
                });
                Ok(())
            })?;
            Ok(())
        })?;
        show_dialog(&create)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_order() {
        let mut names = vec!["beta", "Alpha", "alpha", "B", "a"];
        names.sort_by(|a, b| case_insensitive_then_natural(a, b));
        assert_eq!(names, vec!["a", "Alpha", "alpha", "B", "beta"]);
    }
}
