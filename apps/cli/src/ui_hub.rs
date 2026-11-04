//! Port of `UiHub.java` (package-private in Java): in-memory UI rendezvous
//! for a proxy that is the reachable root of a tree.

use notemaker_core::clock::EpochMillis;
use notemaker_core::error::require;
use notemaker_core::{json, object, Error, Map, Result, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// `UiHub`. Java `synchronized` methods with `wait`/`notifyAll` become one
/// `Mutex` plus a `Condvar`.
pub struct UiHub {
    state: Mutex<HubState>,
    changed: Condvar,
}

#[derive(Default)]
struct HubState {
    sessions: indexmap::IndexMap<String, Map>,
    pending: indexmap::IndexMap<String, Pending>,
    completed: HashMap<String, i64>,
}

/// `UiHub.Pending`.
struct Pending {
    session: String,
    proxy: String,
    verb: String,
    action: String,
    input: Map,
    result: Option<Map>,
}

/// `System.currentTimeMillis()`.
fn now_millis() -> i64 {
    EpochMillis::now().0
}

fn id<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a str> {
    match value {
        Some(Value::String(s)) if !s.is_empty() && json::jlen(s) <= 200 => Ok(s),
        _ => Err(Error::illegal_argument(format!("Invalid {}", name))),
    }
}

fn expires(record: &Map) -> i64 {
    json::as_long(record.get("expires")).unwrap_or(0)
}

impl HubState {
    /// Returns whether waiters must be notified.
    fn expire(&mut self, now: i64) -> bool {
        let mut notify = false;
        let gone: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, r)| expires(r) <= now)
            .map(|(k, _)| k.clone())
            .collect();
        for session in gone {
            self.sessions.shift_remove(&session);
            self.fail_session(&session);
            notify = true;
        }
        self.completed.retain(|_, until| *until > now);
        notify
    }
    fn fail_session(&mut self, session: &str) {
        for request in self.pending.values_mut() {
            if request.session == session && request.result.is_none() {
                request.result = Some(object! {"error" => "App session disconnected"});
            }
        }
    }
}

impl UiHub {
    /// `LEASE_MS`.
    pub const LEASE_MS: i64 = 12000;
    /// `RESULT_MS`.
    pub const RESULT_MS: i64 = 60000;

    pub fn new() -> UiHub {
        UiHub {
            state: Mutex::new(HubState::default()),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HubState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `list()`: `{sessions: [...sorted by session], scope: "root"}`.
    pub fn list(&self) -> Map {
        let mut state = self.lock();
        if state.expire(now_millis()) {
            self.changed.notify_all();
        }
        let mut values: Vec<Value> = state
            .sessions
            .values()
            .cloned()
            .map(Value::Object)
            .collect();
        values.sort_by_key(|v| json::java_string(v.get("session")));
        object! {"sessions" => values, "scope" => "root"}
    }

    /// `heartbeat(body)`: `{commands, acknowledged, serverTime}`.
    pub fn heartbeat(&self, body: &Map) -> Result<Map> {
        let mut state = self.lock();
        let proxy = id(body.get("proxyId"), "proxy ID")?.to_string();
        let reported = json::list(body.get("sessions").unwrap_or(&Value::Null))?.clone();
        let empty = Value::Array(Vec::new());
        let results = json::list(json::get_or_default(body, "results", &empty))?.clone();
        require(
            reported.len() <= 32 && results.len() <= 64,
            "Invalid UI heartbeat",
        )?;
        let now = now_millis();
        let mut notify = state.expire(now);
        let mut current = HashSet::new();
        for value in &reported {
            let record_in = json::map(value)?;
            let session = id(record_in.get("session"), "session ID")?.to_string();
            if let Some(old) = state.sessions.get(&session) {
                require(
                    old.get("proxyId").and_then(Value::as_str) == Some(proxy.as_str()),
                    "Session ID belongs to another proxy",
                )?;
            }
            current.insert(session.clone());
            let mut record = json::map(&json::copy(value))?.clone();
            record.insert("proxyId".into(), Value::from(proxy.clone()));
            record.insert("lastSeen".into(), Value::from(now));
            record.insert("expires".into(), Value::from(now + UiHub::LEASE_MS));
            state.sessions.insert(session, record);
        }
        let stale: Vec<String> = state
            .sessions
            .iter()
            .filter(|(k, r)| {
                r.get("proxyId").and_then(Value::as_str) == Some(proxy.as_str())
                    && !current.contains(*k)
            })
            .map(|(k, _)| k.clone())
            .collect();
        for session in stale {
            state.sessions.shift_remove(&session);
            state.fail_session(&session);
            notify = true;
        }
        let mut acknowledged = Vec::new();
        let mut commands = Vec::new();
        for value in &results {
            let result = json::map(value)?;
            let key = id(result.get("requestId"), "request ID")?.to_string();
            let owned = state.pending.get(&key).is_some_and(|r| r.proxy == proxy);
            if owned {
                let outcome = if result.contains_key("error") {
                    object! {"error" => result.get("error").cloned().unwrap_or(Value::Null)}
                } else {
                    object! {"value" => result.get("value").cloned().unwrap_or(Value::Null)}
                };
                state.pending.get_mut(&key).expect("pending request").result = Some(outcome);
                state.completed.insert(key.clone(), now + UiHub::RESULT_MS);
                acknowledged.push(Value::from(key));
                notify = true;
            } else if state.completed.contains_key(&key) {
                acknowledged.push(Value::from(key));
            }
        }
        for (key, request) in &state.pending {
            if request.proxy == proxy && request.result.is_none() {
                commands.push(Value::Object(object! {
                    "requestId" => key.clone(), "session" => request.session.clone(), "verb" => request.verb.clone(),
                    "action" => request.action.clone(), "input" => Value::Object(request.input.clone())
                }));
            }
        }
        if notify {
            self.changed.notify_all();
        }
        Ok(object! {"commands" => commands, "acknowledged" => acknowledged, "serverTime" => now})
    }

    /// `command(body)`: blocks up to 32 s for the app's result; `{result}`.
    pub fn command(&self, body: &Map) -> Result<Map> {
        let mut state = self.lock();
        let session = id(body.get("session"), "session ID")?.to_string();
        let verb = id(body.get("verb"), "UI verb")?.to_string();
        require(
            ["state", "actions", "action"].contains(&verb.as_str()),
            "Invalid UI verb",
        )?;
        let empty = Value::from("");
        let action = match json::get_or_default(body, "action", &empty) {
            Value::String(s) if json::jlen(s) <= 200 => s.clone(),
            _ => return Err(Error::illegal_argument("Invalid UI action")),
        };
        require(
            verb != "action" || !action.is_empty(),
            "UI action name is required",
        )?;
        let empty_map = Value::Object(Map::new());
        let input = json::map(json::get_or_default(body, "input", &empty_map))?.clone();
        if state.expire(now_millis()) {
            self.changed.notify_all();
        }
        let owner = state.sessions.get(&session).ok_or_else(|| {
            Error::illegal_argument(format!("App session is not connected: {}", session))
        })?;
        let proxy = json::java_string(owner.get("proxyId"));
        let key = String::from(notemaker_core::ids::Uuid::random());
        state.pending.insert(
            key.clone(),
            Pending {
                session,
                proxy,
                verb,
                action,
                input,
                result: None,
            },
        );
        let until = Instant::now() + Duration::from_secs(32);
        let outcome = loop {
            if let Some(result) = state.pending.get(&key).and_then(|r| r.result.clone()) {
                break Ok(result);
            }
            let now = Instant::now();
            if now >= until {
                break Err(Error::illegal_argument(
                    "UI command timed out; inspect session state before retrying",
                ));
            }
            let wait = (until - now).min(Duration::from_millis(1000));
            state = self
                .changed
                .wait_timeout(state, wait)
                .unwrap_or_else(|e| e.into_inner())
                .0;
            if state.expire(now_millis()) {
                self.changed.notify_all();
            }
        };
        state.pending.shift_remove(&key);
        state.completed.insert(key, now_millis() + UiHub::RESULT_MS);
        let result = outcome?;
        if let Some(error) = result.get("error") {
            return Err(Error::illegal_argument(json::java_string(Some(error))));
        }
        Ok(object! {"result" => result.get("value").cloned().unwrap_or(Value::Null)})
    }
}

impl Default for UiHub {
    fn default() -> UiHub {
        UiHub::new()
    }
}
