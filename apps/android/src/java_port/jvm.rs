//! Generic JNI plumbing under [`crate::jx`]: the `JavaVM` and per-thread
//! `JNIEnv`, class resolution through the app class loader, global and local
//! references ([`Obj`], [`Local`]), call argument and result marshalling
//! ([`Arg`], [`Ret`]), Java strings, primitive arrays and `java.lang` boxing.
//!
//! These handle raw JNI values (`JObject`, `jobject`, `JNIEnv`, primitive
//! `jint`/`jfloat` results) for every module, so they live in `java_port`
//! (ADR 0030). The checked call layer with named classes, members and
//! signatures is [`crate::jx`], which re-exports these names.

use crate::jx::{take_exception, JResult, JavaError};
use jni::objects::{GlobalRef, JClass, JObject, JString, JValueOwned};
use jni::signature::{Primitive, ReturnType};
use jni::sys::{jobject, jvalue};
use jni::{JNIEnv, JavaVM};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock};

pub type Local = JObject<'static>;

static VM: OnceLock<JavaVM> = OnceLock::new();
static LOADER: OnceLock<GlobalRef> = OnceLock::new();
static CLASSES: OnceLock<Mutex<HashMap<String, GlobalRef>>> = OnceLock::new();

pub(crate) fn jni_err(e: jni::errors::Error) -> JavaError {
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
fn vm() -> &'static JavaVM {
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
impl<'a> From<notemaker_core::style::ArgbColor> for Arg<'a> { fn from(v: notemaker_core::style::ArgbColor) -> Self { Arg::Int(v.0) } }
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

pub(crate) fn to_jvalues(env: &mut JNIEnv<'static>, args: &[Arg]) -> JResult<(Vec<jvalue>, Vec<Local>)> {
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

pub(crate) fn return_type(sig: &str) -> ReturnType {
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
pub(crate) fn class_ref(c: &Obj) -> JClass<'static> {
    unsafe { JClass::from_raw(c.raw()) }
}

pub(crate) fn check<T>(r: jni::errors::Result<T>) -> JResult<T> {
    let env = env();
    if env.exception_check().unwrap_or(false) {
        return Err(take_exception().unwrap());
    }
    r.map_err(jni_err)
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
    jcall!(o, "java/lang/Object", "toString", "()Ljava/lang/String;").ok()?.string()
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
    jstatic!("java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;", v).unwrap().some()
}
pub fn boxed_int(v: i32) -> Obj {
    jstatic!("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;", v).unwrap().some()
}
pub fn boxed_long(v: i64) -> Obj {
    jstatic!("java/lang/Long", "valueOf", "(J)Ljava/lang/Long;", v).unwrap().some()
}
pub fn unbox_int(o: &JObject) -> i32 {
    jcall!(o, "java/lang/Number", "intValue", "()I").map(|r| r.int()).unwrap_or(0)
}
pub fn unbox_long(o: &JObject) -> i64 {
    jcall!(o, "java/lang/Number", "longValue", "()J").map(|r| r.long()).unwrap_or(0)
}
pub fn unbox_float(o: &JObject) -> f32 {
    jcall!(o, "java/lang/Number", "floatValue", "()F").map(|r| r.float()).unwrap_or(0.0)
}
pub fn unbox_bool(o: &JObject) -> bool {
    jcall!(o, "java/lang/Boolean", "booleanValue", "()Z").map(|r| r.bool()).unwrap_or(false)
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
