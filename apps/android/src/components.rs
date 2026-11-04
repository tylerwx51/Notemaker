//! Routes manifest component callbacks (Activity, Service, Instrumentation)
//! from the shim to their Rust implementations.

use crate::jx::{self, CallbackArgs, JResult, Local, Reply};

notemaker_core::string_newtype! {
    /// A component lifecycle callback name from the shim (`onCreate`,
    /// `onStartCommand`, `onDestroy`, …).
    pub struct ComponentEventName;
}

/// One `Native.component(component, event, args)` call from the shim.
#[derive(Clone, Copy)]
pub struct ComponentEvent<'a> {
    /// The Activity, Service or Instrumentation (a local reference).
    pub component: &'a Local,
    pub name: &'a ComponentEventName,
    pub args: &'a CallbackArgs,
}

pub fn dispatch(event: &ComponentEvent) -> JResult<Reply> {
    let component = event.component;
    let class = class_name(component);
    match class.as_str() {
        "com.notemaker.MainActivity" => crate::main_activity::component_event(event),
        "com.notemaker.ReplicaService" => crate::replica_service::component_event(event),
        "com.notemaker.Integration" => crate::integration::component_event(event),
        other => Err(jx::JavaError::new("java.lang.IllegalStateException", format!("Unknown component {other}"))),
    }
}

fn class_name(o: &Local) -> String {
    jcall!(o, "java/lang/Object", "getClass", "()Ljava/lang/Class;")
        .and_then(|c| {
            let c = c.obj().unwrap();
            jcall!(c, "java/lang/Class", "getName", "()Ljava/lang/String;")
        })
        .ok()
        .and_then(|r| r.string())
        .unwrap_or_default()
}
