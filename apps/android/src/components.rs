//! Routes manifest component callbacks (Activity, Service, Instrumentation)
//! from the shim to their Rust implementations.

use crate::jx::{self, JResult, Local, Reply};

pub fn dispatch(component: &Local, event: &str, args: &[Local]) -> JResult<Reply> {
    let class = class_name(component);
    match class.as_str() {
        "com.notemaker.MainActivity" => crate::main_activity::component_event(component, event, args),
        "com.notemaker.ReplicaService" => crate::replica_service::component_event(component, event, args),
        "com.notemaker.Integration" => crate::integration::component_event(component, event, args),
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
