//! NoteMaker Android application (Rust port of apps/android/src/com/notemaker).
//! The Java shim in `shim/` forwards Android callbacks to the entry points here.

// Infrastructure (verified on the emulator; see docs/design/rust-port.md).
#[macro_use]
pub mod jx;
pub mod components;
pub mod field;
#[cfg_attr(dylint_lib = "notemaker_lints", allow(raw_type_in_signature))]
pub mod java_port;
pub use java_port::{concurrent, monitor, org_json};

// Port support: error type, java.util.concurrent, constants, JNI Canvas
// backend for notemaker_cli::canvas::Canvas, widget helpers.
pub mod android_canvas;
pub mod android_consts;
pub mod error;
pub mod ui;

// One module per Java class in apps/android/src/com/notemaker
// (work packages: docs/design/android-port-plan.md).
pub mod agent_ui;
pub mod animation_player;
pub mod app_replica;
pub mod checkpoint_performance;
pub mod coverage_check;
pub mod document_browser;
pub mod editor_ui;
pub mod erase_performance;
pub mod ink_cache;
pub mod integration;
pub mod interaction_performance;
pub mod main_activity;
pub mod network;
pub mod performance;
pub mod replica_service;
pub mod stroke_check;
pub mod stroke_crossings;
pub mod tile_ink_cache;
pub mod tool_display;
pub mod ui_actions;
pub mod viewport_check;

// JNI exports (ADR 0030): `#[no_mangle]` exports the symbols whatever their
// Rust visibility, so these stay private shims that turn JNI values into
// the checked types of `jx` and `components`.

use components::ComponentEvent;
use jni::objects::{JClass, JObject, JObjectArray, JString};
use jni::sys::{jint, jlong, jobject, JNI_VERSION_1_6};
use jni::{JNIEnv, JavaVM};
use jx::{CallbackArgs, CallbackMethod, JResult, JavaError, NativeHandle, Reply};

#[no_mangle]
extern "system" fn JNI_OnLoad(vm: JavaVM, _: *mut std::ffi::c_void) -> jint {
    java_port::jvm::init(vm);
    JNI_VERSION_1_6
}

fn java_string(env: &mut JNIEnv, value: &JString) -> String {
    env.get_string(value).map(|s| s.into()).unwrap_or_default()
}

fn args_vec(env: &mut JNIEnv, args: &JObjectArray) -> CallbackArgs {
    if args.is_null() {
        return CallbackArgs(Vec::new());
    }
    let n = env.get_array_length(args).unwrap_or(0);
    CallbackArgs((0..n).map(|i| env.get_object_array_element(args, i).map(|o| unsafe { jx::local(o.into_raw()) }).unwrap_or_else(|_| JObject::null())).collect())
}

/// Converts a callback reply into a boxed Java object for the shim.
fn reply_to_java(r: Reply) -> jobject {
    let env = jx::env();
    let o = match r {
        Reply::Void => return std::ptr::null_mut(),
        Reply::Bool(b) => jx::boxed_bool(b),
        Reply::Int(i) => jx::boxed_int(i),
        Reply::Obj(None) => return std::ptr::null_mut(),
        Reply::Obj(Some(o)) => o,
    };
    env.new_local_ref(o.as_obj()).unwrap().into_raw()
}

/// Runs a native entry point, converting Rust errors and panics into Java
/// exceptions so that failures crash or propagate as the Java code did.
/// `name` (the callback key or component event) labels panics.
fn guard<T: Default>(name: &str, f: impl FnOnce() -> JResult<T>) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            jx::throw_to_java(&e);
            T::default()
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panic".into());
            jx::throw_to_java(&JavaError::new("java.lang.RuntimeException", format!("Rust panic in {name}: {msg}")));
            T::default()
        }
    }
}

#[no_mangle]
extern "system" fn Java_com_notemaker_Native_invoke<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, handle: jlong, method: JString<'l>, args: JObjectArray<'l>) -> jobject {
    let method = CallbackMethod::from(java_string(&mut env, &method));
    let args = args_vec(&mut env, &args);
    let reply = guard(&method, || jx::dispatch(NativeHandle(handle), &method, &args).map(Some));
    match reply {
        Some(r) => reply_to_java(r),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
extern "system" fn Java_com_notemaker_Native_release<'l>(_: JNIEnv<'l>, _: JClass<'l>, handle: jlong) {
    jx::release(NativeHandle(handle));
}

#[no_mangle]
extern "system" fn Java_com_notemaker_Native_component<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, component: JObject<'l>, event: JString<'l>, args: JObjectArray<'l>) -> jobject {
    let event = components::ComponentEventName::from(java_string(&mut env, &event));
    let args = args_vec(&mut env, &args);
    java_port::jvm::set_class_loader(&component);
    let component = unsafe { jx::local(component.into_raw()) };
    let reply = guard(&event, || components::dispatch(&ComponentEvent { component: &component, name: &event, args: &args }).map(Some));
    match reply {
        Some(r) => reply_to_java(r),
        None => std::ptr::null_mut(),
    }
}
