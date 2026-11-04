//! Thin JNI layer used by the one-to-one port of the Android Java sources.
//!
//! Every Java call names its declaring class, method and JNI signature as
//! string literals (`jcall!`, `jnew!`, `jstatic!`, `jget!`, `jsget!`), so
//! `apps/android/tools/check_jni.py` can verify all of them against
//! `android.jar` and the shim on the host before anything runs on a device.
//!
//! The macros build a [`JavaMember`] (class, member name, signature) and
//! [`JavaArgs`]; the generic JNI plumbing they use (`env`, [`Obj`], [`Arg`],
//! [`Ret`], arrays, boxing) lives in `java_port::jvm` and is re-exported here.
//!
//! Threads: `env()` attaches the current thread permanently on first use.
//! Local references created on Rust-owned threads must be released with
//! `frame(|| ...)` in loops; native callbacks from Java free theirs on return.

use crate::java_port::jvm::{check, class_ref, jni_err, return_type, to_jvalues};
use jni::objects::{JMethodID, JObject, JStaticMethodID, JThrowable, JValueOwned};
use jni::signature::{Primitive, ReturnType};
use jni::JNIEnv;
use notemaker_core::clock::EpochMillis;
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

pub use crate::java_port::jvm::{
    array_element, array_length, boxed_bool, boxed_int, boxed_long, byte_array, env, find_class, float_array, frame, global, int_array, jstr, local,
    new_boolean_array, new_byte_array, new_float_array, new_int_array, new_object_array, new_string_array, rstr, to_string, unbox_bool,
    unbox_float, unbox_int, unbox_long, Arg, AsRaw, Local, Obj, Ret, NULL,
};

static METHODS: OnceLock<Mutex<IdCache>> = OnceLock::new();
static FIELDS: OnceLock<Mutex<IdCache>> = OnceLock::new();

/// Member-ID cache keyed without allocating on lookup (hot drawing paths
/// make thousands of calls per frame).
#[derive(Default)]
struct IdCache(HashMap<u64, Vec<(String, String, String, bool, usize)>>);
impl IdCache {
    fn hash(class: &str, name: &str, sig: &str, is_static: bool) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (class, name, sig, is_static).hash(&mut h);
        h.finish()
    }
    fn get(&self, class: &str, name: &str, sig: &str, is_static: bool) -> Option<usize> {
        self.0
            .get(&Self::hash(class, name, sig, is_static))?
            .iter()
            .find(|e| e.0 == class && e.1 == name && e.2 == sig && e.3 == is_static)
            .map(|e| e.4)
    }
    fn insert(&mut self, class: &str, name: &str, sig: &str, is_static: bool, id: usize) {
        self.0.entry(Self::hash(class, name, sig, is_static)).or_default().push((class.into(), name.into(), sig.into(), is_static, id));
    }
}

/// A Java exception or JNI failure. `class` is the binary Java class name
/// (`java.io.IOException`) when a Java throwable was pending.
#[derive(Clone, Debug)]
pub struct JavaError {
    pub class: String,
    pub message: Option<String>,
    /// `Throwable.toString()`, e.g. `java.io.IOException: closed`.
    pub text: String,
    /// `Log.getStackTraceString(throwable)` when available.
    pub stack: String,
    /// Retained throwable, rethrown by `throw_to_java`.
    pub throwable: Option<Obj>,
}
impl fmt::Display for JavaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
impl std::error::Error for JavaError {}
pub type JResult<T> = Result<T, JavaError>;

/// A Java class in JNI internal form (`android/widget/TextView`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JavaClass(pub &'static str);
/// A Java method or field name (`setText`, `left`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MemberName(pub &'static str);
/// A JNI type descriptor: a method signature (`(Ljava/lang/CharSequence;)V`)
/// or a field type (`F`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JniSignature(pub &'static str);
/// One method or field of a Java class, as `jcall!`/`jget!`/… name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JavaMember {
    pub class: JavaClass,
    pub name: MemberName,
    pub signature: JniSignature,
}
/// A constructor (`<init>`) of a Java class, as `jnew!` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct JavaConstructor {
    pub class: JavaClass,
    pub signature: JniSignature,
}
/// The arguments of one JNI call, in declaration order.
#[derive(Clone, Copy)]
pub struct JavaArgs<'a>(pub &'a [Arg<'a>]);

fn method_id(class: &str, name: &str, sig: &str, is_static: bool) -> JResult<usize> {
    let cache = METHODS.get_or_init(Default::default);
    if let Some(id) = cache.lock().unwrap().get(class, name, sig, is_static) {
        return Ok(id);
    }
    let c = find_class(class)?;
    let mut env = env();
    let id = if is_static {
        env.get_static_method_id(&class_ref(&c), name, sig).map_err(jni_err)?.into_raw() as usize
    } else {
        env.get_method_id(&class_ref(&c), name, sig).map_err(jni_err)?.into_raw() as usize
    };
    cache.lock().unwrap().insert(class, name, sig, is_static, id);
    Ok(id)
}

/// Instance call through the method declared on `member.class`.
pub fn call(target: &impl AsRaw, member: JavaMember, args: JavaArgs) -> JResult<Ret> {
    let (obj, class, name, sig, args) = (target.as_raw_obj(), member.class.0, member.name.0, member.signature.0, args.0);
    if obj.is_null() {
        return Err(JavaError::new("java.lang.NullPointerException", format!("{class}.{name} on null")));
    }
    let id = method_id(class, name, sig, false)?;
    let mut env = env();
    let (values, keep) = to_jvalues(&mut env, args)?;
    let target = unsafe { JObject::from_raw(obj) };
    let result = unsafe { env.call_method_unchecked(&target, JMethodID::from_raw(id as _), return_type(sig), &values) };
    std::mem::forget(target);
    for k in keep {
        let _ = env.delete_local_ref(k);
    }
    check(result.map(Ret))
}
pub fn call_static(member: JavaMember, args: JavaArgs) -> JResult<Ret> {
    let (class, name, sig, args) = (member.class.0, member.name.0, member.signature.0, args.0);
    let id = method_id(class, name, sig, true)?;
    let c = find_class(class)?;
    let mut env = env();
    let (values, keep) = to_jvalues(&mut env, args)?;
    let result = unsafe { env.call_static_method_unchecked(&class_ref(&c), JStaticMethodID::from_raw(id as _), return_type(sig), &values) };
    for k in keep {
        let _ = env.delete_local_ref(k);
    }
    check(result.map(Ret))
}
/// Non-virtual call (`super.method(...)` from a shim subclass).
pub fn call_nonvirtual(target: &impl AsRaw, member: JavaMember, args: JavaArgs) -> JResult<Ret> {
    let (obj, class, name, sig, args) = (target.as_raw_obj(), member.class.0, member.name.0, member.signature.0, args.0);
    let id = method_id(class, name, sig, false)?;
    let c = find_class(class)?;
    let mut env = env();
    let (values, keep) = to_jvalues(&mut env, args)?;
    let ret = return_type(sig);
    let raw = env.get_raw();
    let cls = c.raw();
    let mid = id as jni::sys::jmethodID;
    let p = values.as_ptr();
    let value = unsafe {
        let f = &**raw;
        match ret {
            ReturnType::Object | ReturnType::Array => JValueOwned::Object(JObject::from_raw((f.CallNonvirtualObjectMethodA.unwrap())(raw, obj, cls, mid, p))),
            ReturnType::Primitive(Primitive::Boolean) => JValueOwned::Bool((f.CallNonvirtualBooleanMethodA.unwrap())(raw, obj, cls, mid, p)),
            ReturnType::Primitive(Primitive::Int) => JValueOwned::Int((f.CallNonvirtualIntMethodA.unwrap())(raw, obj, cls, mid, p)),
            ReturnType::Primitive(Primitive::Long) => JValueOwned::Long((f.CallNonvirtualLongMethodA.unwrap())(raw, obj, cls, mid, p)),
            ReturnType::Primitive(Primitive::Float) => JValueOwned::Float((f.CallNonvirtualFloatMethodA.unwrap())(raw, obj, cls, mid, p)),
            ReturnType::Primitive(Primitive::Double) => JValueOwned::Double((f.CallNonvirtualDoubleMethodA.unwrap())(raw, obj, cls, mid, p)),
            ReturnType::Primitive(Primitive::Void) => {
                (f.CallNonvirtualVoidMethodA.unwrap())(raw, obj, cls, mid, p);
                JValueOwned::Void
            }
            _ => panic!("unsupported nonvirtual return type {sig}"),
        }
    };
    for k in keep {
        let _ = env.delete_local_ref(k);
    }
    check(Ok(Ret(value)))
}
pub fn new(constructor: JavaConstructor, args: JavaArgs) -> JResult<Obj> {
    let (class, sig, args) = (constructor.class.0, constructor.signature.0, args.0);
    let id = method_id(class, "<init>", sig, false)?;
    let c = find_class(class)?;
    let mut env = env();
    let (values, keep) = to_jvalues(&mut env, args)?;
    let result = unsafe { env.new_object_unchecked(&class_ref(&c), JMethodID::from_raw(id as _), &values) };
    for k in keep {
        let _ = env.delete_local_ref(k);
    }
    let local = check(result)?;
    let g = global(&local).unwrap();
    let _ = env.delete_local_ref(local);
    Ok(g)
}
fn field_id(class: &str, name: &str, sig: &str, is_static: bool) -> JResult<usize> {
    let cache = FIELDS.get_or_init(Default::default);
    if let Some(id) = cache.lock().unwrap().get(class, name, sig, is_static) {
        return Ok(id);
    }
    let c = find_class(class)?;
    let mut env = env();
    let id = if is_static {
        env.get_static_field_id(&class_ref(&c), name, sig).map_err(jni_err)?.into_raw() as usize
    } else {
        env.get_field_id(&class_ref(&c), name, sig).map_err(jni_err)?.into_raw() as usize
    };
    cache.lock().unwrap().insert(class, name, sig, is_static, id);
    Ok(id)
}
pub fn get_field(target: &impl AsRaw, field: JavaMember) -> JResult<Ret> {
    let (obj, class, name, sig) = (target.as_raw_obj(), field.class.0, field.name.0, field.signature.0);
    let id = field_id(class, name, sig, false)?;
    let mut env = env();
    let target = unsafe { JObject::from_raw(obj) };
    let r = unsafe { env.get_field_unchecked(&target, jni::objects::JFieldID::from_raw(id as _), return_type(&format!("(){sig}"))) };
    std::mem::forget(target);
    check(r.map(Ret))
}
pub fn get_static(field: JavaMember) -> JResult<Ret> {
    let (class, name, sig) = (field.class.0, field.name.0, field.signature.0);
    let id = field_id(class, name, sig, true)?;
    let c = find_class(class)?;
    let mut env = env();
    let ret = match return_type(&format!("(){sig}")) {
        ReturnType::Primitive(p) => jni::signature::JavaType::Primitive(p),
        _ => jni::signature::JavaType::Object(sig.to_string()),
    };
    let r = unsafe { env.get_static_field_unchecked(&class_ref(&c), jni::objects::JStaticFieldID::from_raw(id as _), ret) };
    check(r.map(Ret))
}

/// Instance field write (`obj.name = value`); used through `jset!`.
pub fn set_field(target: &impl AsRaw, field: JavaMember, value: Arg) -> JResult<()> {
    let (obj, class, name, sig) = (target.as_raw_obj(), field.class.0, field.name.0, field.signature.0);
    let id = field_id(class, name, sig, false)?;
    let mut env = env();
    let (values, keep) = to_jvalues(&mut env, std::slice::from_ref(&value))?;
    let raw = env.get_raw();
    let fid = id as jni::sys::jfieldID;
    let v = values[0];
    unsafe {
        let f = &**raw;
        match sig.as_bytes()[0] {
            b'L' | b'[' => (f.SetObjectField.unwrap())(raw, obj, fid, v.l),
            b'Z' => (f.SetBooleanField.unwrap())(raw, obj, fid, v.z),
            b'B' => (f.SetByteField.unwrap())(raw, obj, fid, v.b),
            b'C' => (f.SetCharField.unwrap())(raw, obj, fid, v.c),
            b'S' => (f.SetShortField.unwrap())(raw, obj, fid, v.s),
            b'I' => (f.SetIntField.unwrap())(raw, obj, fid, v.i),
            b'J' => (f.SetLongField.unwrap())(raw, obj, fid, v.j),
            b'F' => (f.SetFloatField.unwrap())(raw, obj, fid, v.f),
            b'D' => (f.SetDoubleField.unwrap())(raw, obj, fid, v.d),
            _ => panic!("bad JNI field signature {sig}"),
        }
    }
    for k in keep {
        let _ = env.delete_local_ref(k);
    }
    check(Ok(()))
}

/// Clears and returns the pending Java exception, if any.
pub(crate) fn take_exception() -> Option<JavaError> {
    let mut env = env();
    if !env.exception_check().unwrap_or(false) {
        return None;
    }
    let t = env.exception_occurred().ok()?;
    env.exception_clear().ok()?;
    // Every intermediate is a local; free them (see `promote`).
    fn text_of(env: &mut JNIEnv<'static>, v: jni::errors::Result<JValueOwned<'static>>) -> Option<String> {
        let o = v.and_then(|v| v.l()).ok()?;
        if o.is_null() {
            return None;
        }
        let s = rstr(&o);
        let _ = env.delete_local_ref(o);
        Some(s)
    }
    let class = match env.call_method(&t, "getClass", "()Ljava/lang/Class;", &[]).and_then(|c| c.l()) {
        Ok(c) => {
            let r = env.call_method(&c, "getName", "()Ljava/lang/String;", &[]);
            let name = text_of(&mut env, r).unwrap_or_default();
            let _ = env.delete_local_ref(c);
            name
        }
        Err(_) => String::new(),
    };
    let r = env.call_method(&t, "getMessage", "()Ljava/lang/String;", &[]);
    let message = text_of(&mut env, r);
    let r = env.call_method(&t, "toString", "()Ljava/lang/String;", &[]);
    let text = text_of(&mut env, r).unwrap_or_default();
    let r = env.call_static_method("android/util/Log", "getStackTraceString", "(Ljava/lang/Throwable;)Ljava/lang/String;", &[(&t).into()]);
    let stack = text_of(&mut env, r).unwrap_or_default();
    let _ = env.exception_clear();
    let throwable = global(&t);
    let _ = env.delete_local_ref(t);
    Some(JavaError { class, message, text, stack, throwable })
}
/// Throws `error` into Java (used when a native callback fails).
pub fn throw_to_java(error: &JavaError) {
    let mut env = env();
    if let Some(t) = &error.throwable {
        let _ = env.throw(unsafe { JThrowable::from_raw(t.raw()) });
    } else {
        let class = if find_class(&error.class.replace('.', "/")).is_ok() { error.class.replace('.', "/") } else { "java/lang/RuntimeException".into() };
        let message = error.message.clone().unwrap_or_else(|| error.text.clone());
        // `new RuntimeException(message, cause)`: keep the cause in the Java
        // stack trace ("Caused by: ...") as the Java implementation did.
        if let Some(cause) = error.stack.strip_prefix("Caused by: ") {
            match chained(&mut env, &class, &message, cause.trim_end()) {
                Some(t) => {
                    let _ = env.throw(t);
                    return;
                }
                None => {
                    let _ = env.exception_clear();
                }
            }
        }
        let _ = env.throw_new(class, message);
    }
}
fn chained(env: &mut JNIEnv<'static>, class: &str, message: &str, cause: &str) -> Option<JThrowable<'static>> {
    let cause_text = env.new_string(cause).ok()?;
    let cause = env.new_object("java/lang/RuntimeException", "(Ljava/lang/String;)V", &[(&cause_text).into()]).ok()?;
    let text = env.new_string(message).ok()?;
    let t = env.new_object(class, "(Ljava/lang/String;Ljava/lang/Throwable;)V", &[(&text).into(), (&cause).into()]).ok()?;
    Some(JThrowable::from(t))
}

// ---------------------------------------------------------------------------
// Callbacks: com.notemaker.NativeCallback implements every listener interface
// used by the app and forwards `invoke(handle, "method", args)` to Rust.

/// Value returned from a callback to Java.
pub enum Reply {
    Void,
    Bool(bool),
    Int(i32),
    Obj(Option<Obj>),
}
pub type CallbackFn = dyn Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync;
struct Handle(Arc<CallbackFn>);

fn into_handle(f: Arc<CallbackFn>) -> i64 {
    Box::into_raw(Box::new(Handle(f))) as i64
}
/// Creates a Java `NativeCallback` that invokes `f(method_key, args)`.
/// The method key is `name(ParamSimpleNames)`, e.g. `onClick(View)`,
/// `onClick(DialogInterface,int)`, `run()`.
pub fn callback(f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> Obj {
    let h = into_handle(Arc::new(f));
    crate::jnew!("com/notemaker/NativeCallback", "(J)V", h).expect("NativeCallback")
}
/// `Runnable`.
pub fn runnable(f: impl Fn() + Send + Sync + 'static) -> Obj {
    callback(move |_, _| {
        f();
        Ok(Reply::Void)
    })
}
/// `Runnable` whose body may fail; failures are thrown into Java.
pub fn try_runnable(f: impl Fn() -> JResult<()> + Send + Sync + 'static) -> Obj {
    callback(move |_, _| f().map(|_| Reply::Void))
}

/// `Runnable` that runs `f` at most once (Java lambdas posted with
/// `runOnUiThread`/`Handler.post` that consume captured values).
pub fn once_runnable(f: impl FnOnce() + Send + 'static) -> Obj {
    let slot = Mutex::new(Some(f));
    callback(move |_, _| {
        let f = slot.lock().unwrap().take();
        if let Some(f) = f {
            f();
        }
        Ok(Reply::Void)
    })
}
/// `Runnable` whose single run may fail; the failure is thrown into Java.
pub fn try_once_runnable(f: impl FnOnce() -> JResult<()> + Send + 'static) -> Obj {
    let slot = Mutex::new(Some(f));
    callback(move |_, _| {
        let f = slot.lock().unwrap().take();
        match f {
            Some(f) => f().map(|_| Reply::Void),
            None => Ok(Reply::Void),
        }
    })
}

/// Raw handle for the shim's `long handle` constructor argument
/// (`NativeView`, `NativeLinearLayout`, `NativeFrameLayout`,
/// `NativeScaleListener`, `NativeAdapter`). Java releases it in `finalize`.
fn handle(f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> i64 {
    into_handle(Arc::new(f))
}

/// The overrides a `NativeView`/`NativeLinearLayout`/`NativeFrameLayout`
/// forwards to Rust (the shim's `int mask`); combine with `|`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventMask(pub i32);
impl std::ops::BitOr for EventMask {
    type Output = EventMask;
    fn bitor(self, other: EventMask) -> EventMask {
        EventMask(self.0 | other.0)
    }
}
impl<'a> From<EventMask> for Arg<'a> {
    fn from(mask: EventMask) -> Self {
        Arg::Int(mask.0)
    }
}

/// Event mask bits of `NativeView`/`NativeLinearLayout`/`NativeFrameLayout`.
pub mod mask {
    use super::EventMask;
    pub const DRAW: EventMask = EventMask(1);
    pub const DISPATCH_DRAW: EventMask = EventMask(2);
    pub const TOUCH: EventMask = EventMask(4);
    pub const DETACHED: EventMask = EventMask(8);
    pub const SIZE: EventMask = EventMask(16);
    pub const ATTACHED: EventMask = EventMask(32);
}

/// `new NativeView(context, handle, mask)`: a `View` subclass whose masked
/// overrides call `f("onDraw(Canvas)", [this, canvas])` and so on.
///
/// Ownership: the Java view owns `f` (released by `finalize`). A Rust struct
/// captured strongly by `f` must refer to this view (and to other views of the
/// same tree) through [`WeakObj`], otherwise the global reference keeps the
/// view tree alive forever.
pub fn native_view(context: &Obj, mask: EventMask, f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> JResult<Obj> {
    let h = handle(f);
    crate::jnew!("com/notemaker/NativeView", "(Landroid/content/Context;JI)V", context, h, mask)
}
/// `new NativeLinearLayout(context, handle, mask)`; see [`native_view`].
pub fn native_linear_layout(context: &Obj, mask: EventMask, f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> JResult<Obj> {
    let h = handle(f);
    crate::jnew!("com/notemaker/NativeLinearLayout", "(Landroid/content/Context;JI)V", context, h, mask)
}
/// `new NativeFrameLayout(context, handle, mask)`; see [`native_view`].
pub fn native_frame_layout(context: &Obj, mask: EventMask, f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> JResult<Obj> {
    let h = handle(f);
    crate::jnew!("com/notemaker/NativeFrameLayout", "(Landroid/content/Context;JI)V", context, h, mask)
}
/// `new NativeScaleListener(handle)`: keys `onScale(ScaleGestureDetector)`,
/// `onScaleBegin(ScaleGestureDetector)`, `onScaleEnd(ScaleGestureDetector)`.
/// Return `Reply::Bool` to override, anything else for the `super` result.
pub fn scale_listener(f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> JResult<Obj> {
    let h = handle(f);
    crate::jnew!("com/notemaker/NativeScaleListener", "(J)V", h)
}
/// `new NativeAdapter(handle)` (`BaseAdapter`): keys `getCount()`,
/// `getItem(int)`, `getItemId(int)`, `getView(int,View,ViewGroup)`.
pub fn adapter(f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> JResult<Obj> {
    let h = handle(f);
    crate::jnew!("com/notemaker/NativeAdapter", "(J)V", h)
}

/// A JNI weak global reference: does not keep the Java object alive.
/// Use it for Rust structs reachable from a native handle that need to name
/// their own Java view (Java's implicit `this`) or views of the same tree.
#[derive(Clone)]
pub struct WeakObj(Arc<jni::objects::WeakRef>);
impl WeakObj {
    pub fn new(obj: &Obj) -> WeakObj {
        WeakObj(Arc::new(env().new_weak_ref(obj.as_obj()).expect("new weak ref").expect("weak ref to null")))
    }
    /// The object, or `None` after it was garbage collected.
    pub fn upgrade(&self) -> Option<Obj> {
        self.0.upgrade_global(&env()).ok().flatten().map(Obj)
    }
    /// The object; panics if it was collected (a Java `this` cannot be).
    pub fn get(&self) -> Obj {
        self.upgrade().expect("Java object was garbage collected")
    }
}
impl fmt::Debug for WeakObj {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WeakObj")
    }
}

/// A `long handle` the shim holds for a Rust callback (`Native.invoke`,
/// `Native.release`); `0` is no callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeHandle(pub i64);

notemaker_core::string_newtype! {
    /// The callback key the shim passes to `Native.invoke`:
    /// `name(ParamSimpleNames)`, e.g. `onClick(View)` or `run()`.
    pub struct CallbackMethod;
}

/// The arguments of one callback from Java (`Object[] args`).
#[derive(Default)]
pub struct CallbackArgs(pub Vec<Local>);
impl std::ops::Deref for CallbackArgs {
    type Target = [Local];
    fn deref(&self) -> &[Local] {
        &self.0
    }
}

/// Runs the callback behind `handle` (`Native.invoke`).
pub(crate) fn dispatch(handle: NativeHandle, method: &CallbackMethod, args: &CallbackArgs) -> JResult<Reply> {
    let h = unsafe { &*(handle.0 as *const Handle) };
    let f = h.0.clone();
    f(method, args)
}
/// Frees the callback behind `handle` (`Native.release` from `finalize`).
pub(crate) fn release(handle: NativeHandle) {
    let handle = handle.0;
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut Handle) });
    }
}

// ---------------------------------------------------------------------------
// Threads

notemaker_core::string_newtype! {
    /// A thread name (`new Thread(runnable, name)`), shown in traces.
    pub struct ThreadName;
}

/// A delay in milliseconds (`Thread.sleep(ms)`, `Handler.postDelayed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DelayMillis(pub i64);

/// A Java-like thread handle with cooperative interruption.
#[derive(Clone)]
pub struct JThread {
    pub name: String,
    state: Arc<(Mutex<bool>, std::sync::Condvar)>,
}
thread_local! {
    static CURRENT: std::cell::RefCell<Option<JThread>> = const { std::cell::RefCell::new(None) };
}
impl JThread {
    /// Starts a named thread (`new Thread(runnable, name).start()`).
    pub fn spawn(name: &ThreadName, f: impl FnOnce() + Send + 'static) -> JThread {
        let name: &str = name;
        let t = JThread { name: name.into(), state: Arc::new((Mutex::new(false), std::sync::Condvar::new())) };
        let inner = t.clone();
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                CURRENT.with(|c| *c.borrow_mut() = Some(inner));
                f();
            })
            .expect("spawn thread");
        t
    }
    pub fn interrupt(&self) {
        let (m, c) = &*self.state;
        *m.lock().unwrap() = true;
        c.notify_all();
    }
    /// `Thread.currentThread().isInterrupted()`.
    pub fn current_interrupted() -> bool {
        CURRENT.with(|c| c.borrow().as_ref().map(|t| *t.state.0.lock().unwrap()).unwrap_or(false))
    }
    /// `Thread.interrupted()` (clears the flag).
    pub fn interrupted() -> bool {
        CURRENT.with(|c| c.borrow().as_ref().map(|t| std::mem::replace(&mut *t.state.0.lock().unwrap(), false)).unwrap_or(false))
    }
    /// `Thread.sleep(ms)`; returns `Err` (InterruptedException) when interrupted.
    pub fn sleep(delay: DelayMillis) -> JResult<()> {
        let ms = delay.0 as u64;
        let current = CURRENT.with(|c| c.borrow().clone());
        match current {
            None => {
                std::thread::sleep(std::time::Duration::from_millis(ms));
                Ok(())
            }
            Some(t) => {
                let (m, c) = &*t.state;
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
                let mut flag = m.lock().unwrap();
                loop {
                    if *flag {
                        *flag = false;
                        return Err(JavaError::new("java.lang.InterruptedException", "sleep interrupted"));
                    }
                    let now = std::time::Instant::now();
                    if now >= deadline {
                        return Ok(());
                    }
                    flag = c.wait_timeout(flag, deadline - now).unwrap().0;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// android.util.Log

pub mod log {
    use super::*;

    /// An `android.util.Log` tag.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct LogTag(pub &'static str);
    /// The app's general tag.
    pub const NOTEMAKER: LogTag = LogTag("NoteMaker");
    /// Cold-start phase timings (`tests/` startup probes read these).
    pub const STARTUP: LogTag = LogTag("NoteMakerStartup");
    /// Performance harness results.
    pub const PERFORMANCE: LogTag = LogTag("NoteMakerPerformance");
    /// The viewport check's diagnostics.
    pub const VIEWPORT_CHECK: LogTag = LogTag("ViewportCheck");

    fn emit(method: &'static str, tag: LogTag, msg: &str) {
        let log = JavaMember { class: JavaClass("android/util/Log"), name: MemberName(method), signature: JniSignature("(Ljava/lang/String;Ljava/lang/String;)I") };
        let _ = call_static(log, JavaArgs(&[tag.0.into(), msg.into()]));
    }
    pub fn i(tag: LogTag, msg: impl fmt::Display) { emit("i", tag, &msg.to_string()) }
    pub fn d(tag: LogTag, msg: impl fmt::Display) { emit("d", tag, &msg.to_string()) }
    pub fn w(tag: LogTag, msg: impl fmt::Display) { emit("w", tag, &msg.to_string()) }
    pub fn e(tag: LogTag, msg: impl fmt::Display) { emit("e", tag, &msg.to_string()) }
    /// `Log.e(tag, msg, throwable)`: appends the error and its stack.
    pub fn e_err(tag: LogTag, msg: impl fmt::Display, err: &dyn std::fmt::Display) { emit("e", tag, &format!("{msg}\n{err}")) }
    pub fn w_err(tag: LogTag, msg: impl fmt::Display, err: &dyn std::fmt::Display) { emit("w", tag, &format!("{msg}\n{err}")) }
}

/// A `System.nanoTime()` reading: monotonic nanoseconds, only meaningful as
/// a difference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MonotonicNanos(pub i64);

/// A `SystemClock.uptimeMillis()` reading: milliseconds since boot, not
/// counting deep sleep (the time base of `MotionEvent`s and `Handler`s).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UptimeMillis(pub i64);

/// `System.nanoTime()`.
pub fn nano_time() -> MonotonicNanos {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    MonotonicNanos(ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64)
}
/// `System.currentTimeMillis()`.
pub fn current_time_millis() -> EpochMillis {
    EpochMillis::now()
}
/// `SystemClock.uptimeMillis()`.
pub fn uptime_millis() -> UptimeMillis {
    UptimeMillis(crate::jstatic!("android/os/SystemClock", "uptimeMillis", "()J").map(|r| r.long()).unwrap_or(0))
}

// ---------------------------------------------------------------------------
// Macros. Class names use JNI internal form; signatures are JNI descriptors.

/// `jx::JavaMember` from three literals (used by the call macros).
#[macro_export]
#[doc(hidden)]
macro_rules! jmember {
    ($class:literal, $name:literal, $sig:literal) => {
        $crate::jx::JavaMember { class: $crate::jx::JavaClass($class), name: $crate::jx::MemberName($name), signature: $crate::jx::JniSignature($sig) }
    };
}
/// `jcall!(obj, "android/view/View", "setAlpha", "(F)V", 0.5f32)` → `JResult<Ret>`.
#[macro_export]
macro_rules! jcall {
    ($obj:expr, $class:literal, $name:literal, $sig:literal $(, $arg:expr)* $(,)?) => {
        $crate::jx::call(&$obj, $crate::jmember!($class, $name, $sig), $crate::jx::JavaArgs(&[$($crate::jx::Arg::from($arg)),*]))
    };
}
/// `jsuper!(obj, "android/view/View", "onTouchEvent", "(Landroid/view/MotionEvent;)Z", ev)`.
#[macro_export]
macro_rules! jsuper {
    ($obj:expr, $class:literal, $name:literal, $sig:literal $(, $arg:expr)* $(,)?) => {
        $crate::jx::call_nonvirtual(&$obj, $crate::jmember!($class, $name, $sig), $crate::jx::JavaArgs(&[$($crate::jx::Arg::from($arg)),*]))
    };
}
/// `jstatic!("android/graphics/Color", "argb", "(IIII)I", 255, 0, 0, 0)` → `JResult<Ret>`.
#[macro_export]
macro_rules! jstatic {
    ($class:literal, $name:literal, $sig:literal $(, $arg:expr)* $(,)?) => {
        $crate::jx::call_static($crate::jmember!($class, $name, $sig), $crate::jx::JavaArgs(&[$($crate::jx::Arg::from($arg)),*]))
    };
}
/// `jnew!("android/widget/TextView", "(Landroid/content/Context;)V", &ctx)` → `JResult<Obj>`.
#[macro_export]
macro_rules! jnew {
    ($class:literal, $sig:literal $(, $arg:expr)* $(,)?) => {
        $crate::jx::new(
            $crate::jx::JavaConstructor { class: $crate::jx::JavaClass($class), signature: $crate::jx::JniSignature($sig) },
            $crate::jx::JavaArgs(&[$($crate::jx::Arg::from($arg)),*]),
        )
    };
}
/// Instance field read: `jget!(obj, "android/graphics/RectF", "left", "F")`.
#[macro_export]
macro_rules! jget {
    ($obj:expr, $class:literal, $name:literal, $sig:literal) => {
        $crate::jx::get_field(&$obj, $crate::jmember!($class, $name, $sig))
    };
}
/// Instance field write: `jset!(obj, "com/notemaker/MainActivity", "handle", "J", 0i64)`.
#[macro_export]
macro_rules! jset {
    ($obj:expr, $class:literal, $name:literal, $sig:literal, $value:expr) => {
        $crate::jx::set_field(&$obj, $crate::jmember!($class, $name, $sig), $crate::jx::Arg::from($value))
    };
}
/// Static field read: `jsget!("android/graphics/Paint$Style", "FILL", "Landroid/graphics/Paint$Style;")`.
#[macro_export]
macro_rules! jsget {
    ($class:literal, $name:literal, $sig:literal) => {
        $crate::jx::get_static($crate::jmember!($class, $name, $sig))
    };
}
