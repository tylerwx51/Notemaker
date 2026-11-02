//! NoteMaker Android application (Rust port of apps/android/src/com/notemaker).
//! The Java shim in `shim/` forwards Android callbacks to the entry points here.

// Infrastructure (verified on the emulator; see docs/design/rust-port.md).
#[macro_use]
pub mod jx;
pub mod components;
pub mod field;
pub mod monitor;
pub mod org_json;

// Port support: error type, java.util.concurrent, constants, JNI Canvas
// backend for notemaker_cli::canvas::Canvas, widget helpers.
pub mod android_canvas;
pub mod android_consts;
pub mod concurrent;
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
pub mod performance;
pub mod replica_service;
pub mod stroke_check;
pub mod stroke_crossings;
pub mod tile_ink_cache;
pub mod tool_display;
pub mod ui_actions;
pub mod viewport_check;

use jni::objects::{JClass, JObject, JObjectArray, JString};
use jni::sys::{jint, jlong, jobject, JNI_VERSION_1_6};
use jni::{JNIEnv, JavaVM};

#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: JavaVM, _: *mut std::ffi::c_void) -> jint {
    jx::init(vm);
    JNI_VERSION_1_6
}

fn args_vec(env: &mut JNIEnv, args: &JObjectArray) -> Vec<jx::Local> {
    if args.is_null() {
        return Vec::new();
    }
    let n = env.get_array_length(args).unwrap_or(0);
    (0..n).map(|i| env.get_object_array_element(args, i).map(|o| unsafe { jx::local(o.into_raw()) }).unwrap_or_else(|_| JObject::null())).collect()
}

#[no_mangle]
pub extern "system" fn Java_com_notemaker_Native_invoke<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, handle: jlong, method: JString<'l>, args: JObjectArray<'l>) -> jobject {
    let method: String = env.get_string(&method).map(|s| s.into()).unwrap_or_default();
    let args = args_vec(&mut env, &args);
    let reply = jx::guard(&method, || jx::dispatch(handle, &method, &args).map(Some));
    match reply {
        Some(r) => jx::reply_to_java(r),
        None => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "system" fn Java_com_notemaker_Native_release<'l>(_: JNIEnv<'l>, _: JClass<'l>, handle: jlong) {
    jx::release(handle);
}

#[no_mangle]
pub extern "system" fn Java_com_notemaker_Native_component<'l>(mut env: JNIEnv<'l>, _: JClass<'l>, component: JObject<'l>, event: JString<'l>, args: JObjectArray<'l>) -> jobject {
    let event: String = env.get_string(&event).map(|s| s.into()).unwrap_or_default();
    let args = args_vec(&mut env, &args);
    jx::set_class_loader(&component);
    let component = unsafe { jx::local(component.into_raw()) };
    let reply = jx::guard(&event, || components::dispatch(&component, &event, &args).map(Some));
    match reply {
        Some(r) => jx::reply_to_java(r),
        None => std::ptr::null_mut(),
    }
}
