//! Thin JNI layer used by the one-to-one port of the Android Java sources.
//!
//! Every Java call names its declaring class, method and JNI signature as
//! string literals (`jcall!`, `jnew!`, `jstatic!`, `jget!`, `jsget!`), so
//! `apps/android/tools/check_jni.py` can verify all of them against
//! `android.jar` and the shim on the host before anything runs on a device.
//!
//! Threads: `env()` attaches the current thread permanently on first use.
//! Local references created on Rust-owned threads must be released with
//! `frame(|| ...)` in loops; native callbacks from Java free theirs on return.

use jni::objects::{GlobalRef, JClass, JMethodID, JObject, JStaticMethodID, JString, JThrowable, JValueOwned};
use jni::signature::{Primitive, ReturnType};
use jni::sys::{jobject, jvalue};
use jni::{JNIEnv, JavaVM};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

pub type Local = JObject<'static>;

static VM: OnceLock<JavaVM> = OnceLock::new();
static LOADER: OnceLock<GlobalRef> = OnceLock::new();
static CLASSES: OnceLock<Mutex<HashMap<String, GlobalRef>>> = OnceLock::new();
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
impl JavaError {
    pub fn new(class: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        JavaError { class: class.into(), text: format!("{class}: {message}"), message: Some(message), stack: String::new(), throwable: None }
    }
    /// True when the Java throwable is an instance of `class` (binary name,
    /// e.g. `java.io.IOException`), including subclasses.
    pub fn is(&self, class: &str) -> bool {
        match &self.throwable {
            Some(t) => {
                let mut env = env();
                match find_class(&class.replace('.', "/")) {
                    Ok(c) => env.is_instance_of(t.as_obj(), unsafe { &JClass::from_raw(c.as_obj().as_raw()) }).unwrap_or(false),
                    Err(_) => false,
                }
            }
            None => self.class == class,
        }
    }
}
pub type JResult<T> = Result<T, JavaError>;

fn jni_err(e: jni::errors::Error) -> JavaError {
    if let jni::errors::Error::JavaException = e {
        return take_exception().unwrap_or_else(|| JavaError::new("java.lang.RuntimeException", "pending Java exception"));
    }
    JavaError::new("jni.Error", e.to_string())
}

/// Called from `JNI_OnLoad`.
pub fn init(vm: JavaVM) {
    let _ = VM.set(vm);
}
/// Records the application class loader so app classes resolve on any thread.
pub fn set_class_loader(context_or_class: &JObject) {
    if LOADER.get().is_some() {
        return;
    }
    let mut env = env();
    // Any shim object works: its class was loaded by the app class loader
    // (Instrumentation, unlike Context, has no getClassLoader()).
    let class = env.call_method(context_or_class, "getClass", "()Ljava/lang/Class;", &[]).and_then(|v| v.l());
    let loader = match class.and_then(|c| env.call_method(&c, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])).and_then(|v| v.l()) {
        Ok(l) => l,
        Err(_) => {
            let _ = env.exception_clear();
            return;
        }
    };
    let _ = LOADER.set(env.new_global_ref(loader).expect("global loader"));
}
pub fn vm() -> &'static JavaVM {
    VM.get().expect("JNI not initialised")
}
/// The current thread's environment, attaching it permanently if needed.
pub fn env() -> JNIEnv<'static> {
    let vm = vm();
    let raw = match vm.get_env() {
        Ok(e) => e.get_raw(),
        Err(_) => vm.attach_current_thread_permanently().expect("attach thread").get_raw(),
    };
    unsafe { JNIEnv::from_raw(raw).unwrap() }
}

/// Runs `f` inside a JNI local frame, freeing locals it created.
pub fn frame<T>(f: impl FnOnce() -> T) -> T {
    let env = env();
    unsafe {
        let raw = env.get_raw();
        ((**raw).PushLocalFrame.unwrap())(raw, 64);
    }
    let result = f();
    unsafe {
        let raw = env.get_raw();
        ((**raw).PopLocalFrame.unwrap())(raw, std::ptr::null_mut());
    }
    result
}

/// A global reference. Cheap to clone, usable from any thread.
#[derive(Clone)]
pub struct Obj(pub GlobalRef);
impl Obj {
    pub fn as_obj(&self) -> &JObject<'static> {
        // GlobalRef derefs to JObject<'static>.
        unsafe { &*(self.0.as_obj() as *const JObject as *const JObject<'static>) }
    }
    pub fn raw(&self) -> jobject {
        self.0.as_obj().as_raw()
    }
    /// Java reference equality (`==`).
    pub fn same(&self, other: &Obj) -> bool {
        env().is_same_object(self.as_obj(), other.as_obj()).unwrap_or(false)
    }
}
impl fmt::Debug for Obj {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Obj({:?})", self.raw())
    }
}
impl PartialEq for Obj {
    fn eq(&self, other: &Self) -> bool {
        self.same(other)
    }
}
pub fn global(obj: &JObject) -> Option<Obj> {
    if obj.is_null() {
        None
    } else {
        Some(Obj(env().new_global_ref(obj).expect("new global ref")))
    }
}
/// Wraps a raw local reference without taking ownership semantics.
pub unsafe fn local(raw: jobject) -> Local {
    JObject::from_raw(raw)
}

/// One JNI call argument.
pub enum Arg<'a> {
    Null,
    Obj(jobject),
    Str(&'a str),
    String(String),
    Bool(bool),
    Byte(i8),
    Char(u16),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}
impl<'a> From<&'a str> for Arg<'a> { fn from(v: &'a str) -> Self { Arg::Str(v) } }
impl<'a> From<&'a String> for Arg<'a> { fn from(v: &'a String) -> Self { Arg::Str(v) } }
impl<'a> From<String> for Arg<'a> { fn from(v: String) -> Self { Arg::String(v) } }
impl<'a> From<bool> for Arg<'a> { fn from(v: bool) -> Self { Arg::Bool(v) } }
impl<'a> From<i8> for Arg<'a> { fn from(v: i8) -> Self { Arg::Byte(v) } }
impl<'a> From<i16> for Arg<'a> { fn from(v: i16) -> Self { Arg::Short(v) } }
impl<'a> From<i32> for Arg<'a> { fn from(v: i32) -> Self { Arg::Int(v) } }
impl<'a> From<i64> for Arg<'a> { fn from(v: i64) -> Self { Arg::Long(v) } }
impl<'a> From<f32> for Arg<'a> { fn from(v: f32) -> Self { Arg::Float(v) } }
impl<'a> From<f64> for Arg<'a> { fn from(v: f64) -> Self { Arg::Double(v) } }
impl<'a> From<&'a Obj> for Arg<'a> { fn from(v: &'a Obj) -> Self { Arg::Obj(v.raw()) } }
impl<'a, 'b> From<&'a JObject<'b>> for Arg<'a> { fn from(v: &'a JObject<'b>) -> Self { Arg::Obj(v.as_raw()) } }
impl<'a, 'b> From<&'a JString<'b>> for Arg<'a> { fn from(v: &'a JString<'b>) -> Self { Arg::Obj(v.as_raw()) } }
impl<'a> From<&'a Option<Obj>> for Arg<'a> {
    fn from(v: &'a Option<Obj>) -> Self { match v { Some(o) => Arg::Obj(o.raw()), None => Arg::Null } }
}
impl<'a> From<Option<&'a Obj>> for Arg<'a> {
    fn from(v: Option<&'a Obj>) -> Self { match v { Some(o) => Arg::Obj(o.raw()), None => Arg::Null } }
}
impl<'a> From<Option<&'a str>> for Arg<'a> {
    fn from(v: Option<&'a str>) -> Self { match v { Some(o) => Arg::Str(o), None => Arg::Null } }
}
/// Explicit Java `null`.
pub const NULL: Arg<'static> = Arg::Null;

fn to_jvalues(env: &mut JNIEnv<'static>, args: &[Arg]) -> JResult<(Vec<jvalue>, Vec<Local>)> {
    let mut keep = Vec::new();
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        out.push(match a {
            Arg::Null => jvalue { l: std::ptr::null_mut() },
            Arg::Obj(o) => jvalue { l: *o },
            Arg::Str(s) => {
                let j: Local = env.new_string(*s).map_err(jni_err)?.into();
                let v = jvalue { l: j.as_raw() };
                keep.push(j);
                v
            }
            Arg::String(s) => {
                let j: Local = env.new_string(s.as_str()).map_err(jni_err)?.into();
                let v = jvalue { l: j.as_raw() };
                keep.push(j);
                v
            }
            Arg::Bool(b) => jvalue { z: *b as u8 },
            Arg::Byte(b) => jvalue { b: *b },
            Arg::Char(c) => jvalue { c: *c },
            Arg::Short(s) => jvalue { s: *s },
            Arg::Int(i) => jvalue { i: *i },
            Arg::Long(j) => jvalue { j: *j },
            Arg::Float(f) => jvalue { f: *f },
            Arg::Double(d) => jvalue { d: *d },
        });
    }
    Ok((out, keep))
}

/// A call result.
pub struct Ret(pub JValueOwned<'static>);
/// An object result that the caller ignores (Java's `paint.setShader(s);`)
/// still owns a local reference; free it, or long native calls accumulate them.
impl Drop for Ret {
    fn drop(&mut self) {
        if let JValueOwned::Object(o) = std::mem::replace(&mut self.0, JValueOwned::Void) {
            if !o.is_null() {
                let _ = env().delete_local_ref(o);
            }
        }
    }
}
impl Ret {
    fn take(&mut self) -> JValueOwned<'static> {
        std::mem::replace(&mut self.0, JValueOwned::Void)
    }
    pub fn obj(mut self) -> Option<Local> {
        match self.take() {
            JValueOwned::Object(o) if !o.is_null() => Some(o),
            _ => None,
        }
    }
    pub fn global(self) -> Option<Obj> {
        self.obj().map(|o| {
            let g = global(&o);
            let _ = env().delete_local_ref(o);
            g.unwrap()
        })
    }
    /// Required non-null object as a global reference.
    pub fn some(self) -> Obj {
        self.global().expect("unexpected null from Java")
    }
    pub fn string(self) -> Option<String> {
        self.obj().map(|o| {
            let s = rstr(&o);
            let _ = env().delete_local_ref(o);
            s
        })
    }
    pub fn bool(mut self) -> bool { self.take().z().unwrap_or(false) }
    pub fn int(mut self) -> i32 { self.take().i().unwrap_or(0) }
    pub fn long(mut self) -> i64 { self.take().j().unwrap_or(0) }
    pub fn float(mut self) -> f32 { self.take().f().unwrap_or(0.0) }
    pub fn double(mut self) -> f64 { self.take().d().unwrap_or(0.0) }
    pub fn byte(mut self) -> i8 { self.take().b().unwrap_or(0) }
    pub fn char(mut self) -> u16 { self.take().c().unwrap_or(0) }
    pub fn short(mut self) -> i16 { self.take().s().unwrap_or(0) }
    pub fn unit(self) {}
}

fn return_type(sig: &str) -> ReturnType {
    let r = &sig[sig.rfind(')').expect("JNI signature") + 1..];
    match r.as_bytes()[0] {
        b'L' => ReturnType::Object,
        b'[' => ReturnType::Array,
        b'Z' => ReturnType::Primitive(Primitive::Boolean),
        b'B' => ReturnType::Primitive(Primitive::Byte),
        b'C' => ReturnType::Primitive(Primitive::Char),
        b'S' => ReturnType::Primitive(Primitive::Short),
        b'I' => ReturnType::Primitive(Primitive::Int),
        b'J' => ReturnType::Primitive(Primitive::Long),
        b'F' => ReturnType::Primitive(Primitive::Float),
        b'D' => ReturnType::Primitive(Primitive::Double),
        b'V' => ReturnType::Primitive(Primitive::Void),
        _ => panic!("bad JNI signature {sig}"),
    }
}

/// Resolves a class by internal name (`android/widget/TextView`), through the
/// app class loader for application classes.
pub fn find_class(name: &str) -> JResult<Obj> {
    let cache = CLASSES.get_or_init(Default::default);
    if let Some(c) = cache.lock().unwrap().get(name) {
        return Ok(Obj(c.clone()));
    }
    let mut env = env();
    let local: Local = if let (true, Some(loader)) = (name.starts_with("com/notemaker"), LOADER.get()) {
        let dotted = env.new_string(name.replace('/', ".")).map_err(jni_err)?;
        let r = env
            .call_method(loader.as_obj(), "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;", &[(&dotted).into()])
            .map_err(jni_err);
        let _ = env.delete_local_ref(dotted);
        r?.l().map_err(jni_err)?
    } else {
        env.find_class(name).map_err(jni_err)?.into()
    };
    let g = env.new_global_ref(&local).map_err(jni_err)?;
    let _ = env.delete_local_ref(local);
    cache.lock().unwrap().insert(name.to_string(), g.clone());
    Ok(Obj(g))
}
fn class_ref(c: &Obj) -> JClass<'static> {
    unsafe { JClass::from_raw(c.raw()) }
}

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

/// Instance call through the method declared on `class`.
pub fn call(obj: jobject, class: &str, name: &str, sig: &str, args: &[Arg]) -> JResult<Ret> {
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
pub fn call_static(class: &str, name: &str, sig: &str, args: &[Arg]) -> JResult<Ret> {
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
pub fn call_nonvirtual(obj: jobject, class: &str, name: &str, sig: &str, args: &[Arg]) -> JResult<Ret> {
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
pub fn new(class: &str, sig: &str, args: &[Arg]) -> JResult<Obj> {
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
pub fn get_field(obj: jobject, class: &str, name: &str, sig: &str) -> JResult<Ret> {
    let id = field_id(class, name, sig, false)?;
    let mut env = env();
    let target = unsafe { JObject::from_raw(obj) };
    let r = unsafe { env.get_field_unchecked(&target, jni::objects::JFieldID::from_raw(id as _), return_type(&format!("(){sig}"))) };
    std::mem::forget(target);
    check(r.map(Ret))
}
pub fn get_static(class: &str, name: &str, sig: &str) -> JResult<Ret> {
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
pub fn set_field(obj: jobject, class: &str, name: &str, sig: &str, value: Arg) -> JResult<()> {
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

fn check<T>(r: jni::errors::Result<T>) -> JResult<T> {
    let env = env();
    if env.exception_check().unwrap_or(false) {
        return Err(take_exception().unwrap());
    }
    r.map_err(jni_err)
}
/// Clears and returns the pending Java exception, if any.
pub fn take_exception() -> Option<JavaError> {
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
        let _ = env.throw_new(class, error.message.clone().unwrap_or_else(|| error.text.clone()));
    }
}

/// Java string → Rust.
pub fn rstr(o: &JObject) -> String {
    if o.is_null() {
        return String::new();
    }
    let mut env = env();
    let s = unsafe { JString::from_raw(o.as_raw()) };
    let r = env.get_string(&s).map(|v| v.into()).unwrap_or_default();
    std::mem::forget(s);
    r
}
pub fn jstr(s: &str) -> Obj {
    let env = env();
    let l = env.new_string(s).expect("new string");
    let g = global(&l).unwrap();
    let _ = env.delete_local_ref(l);
    g
}
/// `CharSequence.toString()` of any object, or `None` for null.
pub fn to_string(o: &JObject) -> Option<String> {
    if o.is_null() {
        return None;
    }
    call(o.as_raw(), "java/lang/Object", "toString", "()Ljava/lang/String;", &[]).ok()?.string()
}

/// Promotes a local reference to a global and frees the local. Threads that
/// stay inside one native call (the instrumentation thread runs the whole
/// suite inside `Integration.onStart`) never pop their local frame, so a
/// local left behind keeps its object (for example a multi-megabyte `int[]`)
/// reachable for the rest of the run.
fn promote<'a, O: Into<JObject<'a>>>(local: O) -> Obj {
    let local: JObject<'a> = local.into();
    let g = global(&local).unwrap();
    let _ = env().delete_local_ref(local);
    g
}
pub fn new_string_array(values: &[&str]) -> JResult<Obj> {
    let mut env = env();
    let c = find_class("java/lang/String")?;
    let a = env.new_object_array(values.len() as i32, &class_ref(&c), JObject::null()).map_err(jni_err)?;
    for (i, v) in values.iter().enumerate() {
        let s = env.new_string(*v).map_err(jni_err)?;
        env.set_object_array_element(&a, i as i32, &s).map_err(jni_err)?;
        let _ = env.delete_local_ref(s);
    }
    Ok(promote(a))
}
pub fn new_object_array(class: &str, values: &[Option<&Obj>]) -> JResult<Obj> {
    let mut env = env();
    let c = find_class(class)?;
    let a = env.new_object_array(values.len() as i32, &class_ref(&c), JObject::null()).map_err(jni_err)?;
    for (i, v) in values.iter().enumerate() {
        if let Some(v) = v {
            env.set_object_array_element(&a, i as i32, v.as_obj()).map_err(jni_err)?;
        }
    }
    Ok(promote(a))
}
pub fn new_float_array(values: &[f32]) -> JResult<Obj> {
    let env = env();
    let a = env.new_float_array(values.len() as i32).map_err(jni_err)?;
    env.set_float_array_region(&a, 0, values).map_err(jni_err)?;
    Ok(promote(a))
}
pub fn new_int_array(values: &[i32]) -> JResult<Obj> {
    let env = env();
    let a = env.new_int_array(values.len() as i32).map_err(jni_err)?;
    env.set_int_array_region(&a, 0, values).map_err(jni_err)?;
    Ok(promote(a))
}
pub fn new_boolean_array(values: &[bool]) -> JResult<Obj> {
    let env = env();
    let a = env.new_boolean_array(values.len() as i32).map_err(jni_err)?;
    let v: Vec<u8> = values.iter().map(|b| *b as u8).collect();
    env.set_boolean_array_region(&a, 0, &v).map_err(jni_err)?;
    Ok(promote(a))
}
pub fn new_byte_array(values: &[u8]) -> JResult<Obj> {
    let env = env();
    let a = env.byte_array_from_slice(values).map_err(jni_err)?;
    Ok(promote(a))
}
pub fn byte_array(o: &JObject) -> Vec<u8> {
    if o.is_null() {
        return Vec::new();
    }
    let env = env();
    let a = unsafe { jni::objects::JByteArray::from_raw(o.as_raw()) };
    let r = env.convert_byte_array(&a).unwrap_or_default();
    std::mem::forget(a);
    r
}
pub fn int_array(o: &JObject) -> Vec<i32> {
    if o.is_null() {
        return Vec::new();
    }
    let env = env();
    let a = unsafe { jni::objects::JIntArray::from_raw(o.as_raw()) };
    let n = env.get_array_length(&a).unwrap_or(0) as usize;
    let mut v = vec![0; n];
    let _ = env.get_int_array_region(&a, 0, &mut v);
    std::mem::forget(a);
    v
}
pub fn float_array(o: &JObject) -> Vec<f32> {
    if o.is_null() {
        return Vec::new();
    }
    let env = env();
    let a = unsafe { jni::objects::JFloatArray::from_raw(o.as_raw()) };
    let n = env.get_array_length(&a).unwrap_or(0) as usize;
    let mut v = vec![0.0; n];
    let _ = env.get_float_array_region(&a, 0, &mut v);
    std::mem::forget(a);
    v
}
pub fn array_length(o: &JObject) -> i32 {
    let env = env();
    let a = unsafe { jni::objects::JObjectArray::from_raw(o.as_raw()) };
    let n = env.get_array_length(&a).unwrap_or(0);
    std::mem::forget(a);
    n
}
pub fn array_element(o: &JObject, index: i32) -> Option<Obj> {
    let mut env = env();
    let a = unsafe { jni::objects::JObjectArray::from_raw(o.as_raw()) };
    let e = env.get_object_array_element(&a, index).ok();
    std::mem::forget(a);
    let e = e?;
    let g = global(&e);
    let _ = env.delete_local_ref(e);
    g
}

// ---------------------------------------------------------------------------
// Boxing helpers for java.lang wrappers
pub fn boxed_bool(v: bool) -> Obj {
    call_static("java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;", &[Arg::Bool(v)]).unwrap().some()
}
pub fn boxed_int(v: i32) -> Obj {
    call_static("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;", &[Arg::Int(v)]).unwrap().some()
}
pub fn boxed_long(v: i64) -> Obj {
    call_static("java/lang/Long", "valueOf", "(J)Ljava/lang/Long;", &[Arg::Long(v)]).unwrap().some()
}
pub fn unbox_int(o: &JObject) -> i32 {
    call(o.as_raw(), "java/lang/Number", "intValue", "()I", &[]).map(|r| r.int()).unwrap_or(0)
}
pub fn unbox_long(o: &JObject) -> i64 {
    call(o.as_raw(), "java/lang/Number", "longValue", "()J", &[]).map(|r| r.long()).unwrap_or(0)
}
pub fn unbox_float(o: &JObject) -> f32 {
    call(o.as_raw(), "java/lang/Number", "floatValue", "()F", &[]).map(|r| r.float()).unwrap_or(0.0)
}
pub fn unbox_bool(o: &JObject) -> bool {
    call(o.as_raw(), "java/lang/Boolean", "booleanValue", "()Z", &[]).map(|r| r.bool()).unwrap_or(false)
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
    new("com/notemaker/NativeCallback", "(J)V", &[Arg::Long(h)]).expect("NativeCallback")
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
pub fn handle(f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> i64 {
    into_handle(Arc::new(f))
}

/// Event mask bits of `NativeView`/`NativeLinearLayout`/`NativeFrameLayout`.
pub mod mask {
    pub const DRAW: i32 = 1;
    pub const DISPATCH_DRAW: i32 = 2;
    pub const TOUCH: i32 = 4;
    pub const DETACHED: i32 = 8;
    pub const SIZE: i32 = 16;
    pub const ATTACHED: i32 = 32;
}

/// `new NativeView(context, handle, mask)`: a `View` subclass whose masked
/// overrides call `f("onDraw(Canvas)", [this, canvas])` and so on.
///
/// Ownership: the Java view owns `f` (released by `finalize`). A Rust struct
/// captured strongly by `f` must refer to this view (and to other views of the
/// same tree) through [`WeakObj`], otherwise the global reference keeps the
/// view tree alive forever.
pub fn native_view(context: &Obj, mask: i32, f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> JResult<Obj> {
    let h = handle(f);
    crate::jnew!("com/notemaker/NativeView", "(Landroid/content/Context;JI)V", context, h, mask)
}
/// `new NativeLinearLayout(context, handle, mask)`; see [`native_view`].
pub fn native_linear_layout(context: &Obj, mask: i32, f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> JResult<Obj> {
    let h = handle(f);
    crate::jnew!("com/notemaker/NativeLinearLayout", "(Landroid/content/Context;JI)V", context, h, mask)
}
/// `new NativeFrameLayout(context, handle, mask)`; see [`native_view`].
pub fn native_frame_layout(context: &Obj, mask: i32, f: impl Fn(&str, &[Local]) -> JResult<Reply> + Send + Sync + 'static) -> JResult<Obj> {
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

pub(crate) fn dispatch(handle: i64, method: &str, args: &[Local]) -> JResult<Reply> {
    let h = unsafe { &*(handle as *const Handle) };
    let f = h.0.clone();
    f(method, args)
}
pub(crate) fn release(handle: i64) {
    if handle != 0 {
        drop(unsafe { Box::from_raw(handle as *mut Handle) });
    }
}

/// Converts a callback reply into a boxed Java object for the shim.
pub(crate) fn reply_to_java(r: Reply) -> jobject {
    let env = env();
    let o = match r {
        Reply::Void => return std::ptr::null_mut(),
        Reply::Bool(b) => boxed_bool(b),
        Reply::Int(i) => boxed_int(i),
        Reply::Obj(None) => return std::ptr::null_mut(),
        Reply::Obj(Some(o)) => o,
    };
    env.new_local_ref(o.as_obj()).unwrap().into_raw()
}

/// Runs a native entry point, converting Rust errors and panics into Java
/// exceptions so that failures crash or propagate as the Java code did.
pub fn guard<T: Default>(name: &str, f: impl FnOnce() -> JResult<T>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            throw_to_java(&e);
            T::default()
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".into());
            throw_to_java(&JavaError::new("java.lang.RuntimeException", format!("Rust panic in {name}: {msg}")));
            T::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Threads

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
    pub fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> JThread {
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
    pub fn sleep(ms: u64) -> JResult<()> {
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
    fn emit(method: &str, tag: &str, msg: &str) {
        let _ = call_static("android/util/Log", method, "(Ljava/lang/String;Ljava/lang/String;)I", &[tag.into(), msg.into()]);
    }
    pub fn i(tag: &str, msg: &str) { emit("i", tag, msg) }
    pub fn d(tag: &str, msg: &str) { emit("d", tag, msg) }
    pub fn w(tag: &str, msg: &str) { emit("w", tag, msg) }
    pub fn e(tag: &str, msg: &str) { emit("e", tag, msg) }
    /// `Log.e(tag, msg, throwable)`: appends the error and its stack.
    pub fn e_err(tag: &str, msg: &str, err: &dyn std::fmt::Display) { emit("e", tag, &format!("{msg}\n{err}")) }
    pub fn w_err(tag: &str, msg: &str, err: &dyn std::fmt::Display) { emit("w", tag, &format!("{msg}\n{err}")) }
}

/// `System.nanoTime()`.
pub fn nano_time() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as i64 * 1_000_000_000 + ts.tv_nsec as i64
}
/// `System.currentTimeMillis()`.
pub fn current_time_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}
/// `SystemClock.uptimeMillis()`.
pub fn uptime_millis() -> i64 {
    call_static("android/os/SystemClock", "uptimeMillis", "()J", &[]).map(|r| r.long()).unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Macros. Class names use JNI internal form; signatures are JNI descriptors.

/// `jcall!(obj, "android/view/View", "setAlpha", "(F)V", 0.5f32)` → `JResult<Ret>`.
#[macro_export]
macro_rules! jcall {
    ($obj:expr, $class:literal, $name:literal, $sig:literal $(, $arg:expr)* $(,)?) => {
        $crate::jx::call($crate::jx::AsRaw::as_raw_obj(&$obj), $class, $name, $sig, &[$($crate::jx::Arg::from($arg)),*])
    };
}
/// `jsuper!(obj, "android/view/View", "onTouchEvent", "(Landroid/view/MotionEvent;)Z", ev)`.
#[macro_export]
macro_rules! jsuper {
    ($obj:expr, $class:literal, $name:literal, $sig:literal $(, $arg:expr)* $(,)?) => {
        $crate::jx::call_nonvirtual($crate::jx::AsRaw::as_raw_obj(&$obj), $class, $name, $sig, &[$($crate::jx::Arg::from($arg)),*])
    };
}
/// `jstatic!("android/graphics/Color", "argb", "(IIII)I", 255, 0, 0, 0)` → `JResult<Ret>`.
#[macro_export]
macro_rules! jstatic {
    ($class:literal, $name:literal, $sig:literal $(, $arg:expr)* $(,)?) => {
        $crate::jx::call_static($class, $name, $sig, &[$($crate::jx::Arg::from($arg)),*])
    };
}
/// `jnew!("android/widget/TextView", "(Landroid/content/Context;)V", &ctx)` → `JResult<Obj>`.
#[macro_export]
macro_rules! jnew {
    ($class:literal, $sig:literal $(, $arg:expr)* $(,)?) => {
        $crate::jx::new($class, $sig, &[$($crate::jx::Arg::from($arg)),*])
    };
}
/// Instance field read: `jget!(obj, "android/graphics/RectF", "left", "F")`.
#[macro_export]
macro_rules! jget {
    ($obj:expr, $class:literal, $name:literal, $sig:literal) => {
        $crate::jx::get_field($crate::jx::AsRaw::as_raw_obj(&$obj), $class, $name, $sig)
    };
}
/// Instance field write: `jset!(obj, "com/notemaker/MainActivity", "handle", "J", 0i64)`.
#[macro_export]
macro_rules! jset {
    ($obj:expr, $class:literal, $name:literal, $sig:literal, $value:expr) => {
        $crate::jx::set_field($crate::jx::AsRaw::as_raw_obj(&$obj), $class, $name, $sig, $crate::jx::Arg::from($value))
    };
}
/// Static field read: `jsget!("android/graphics/Paint$Style", "FILL", "Landroid/graphics/Paint$Style;")`.
#[macro_export]
macro_rules! jsget {
    ($class:literal, $name:literal, $sig:literal) => {
        $crate::jx::get_static($class, $name, $sig)
    };
}

/// Anything that can be the receiver of a call.
pub trait AsRaw {
    fn as_raw_obj(&self) -> jobject;
}
impl AsRaw for Obj { fn as_raw_obj(&self) -> jobject { self.raw() } }
impl AsRaw for &Obj { fn as_raw_obj(&self) -> jobject { self.raw() } }
impl<'a> AsRaw for JObject<'a> { fn as_raw_obj(&self) -> jobject { self.as_raw() } }
impl<'a> AsRaw for &JObject<'a> { fn as_raw_obj(&self) -> jobject { self.as_raw() } }
impl AsRaw for jobject { fn as_raw_obj(&self) -> jobject { *self } }
