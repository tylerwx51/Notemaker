//! Port of Android's `org.json` (AOSP libcore/json) with Java reference
//! semantics: `JSONObject`/`JSONArray` are shared handles, so mutating one
//! alias is visible through every other, exactly as in the Java code.
//!
//! Formatting follows `JSONStringer`/`JSONObject.numberToString`: integral
//! doubles print as longs (`5.0` → `5`), `-0.0` prints as `-0`, `/` is
//! escaped as `\/`. Parsing follows `JSONTokener` (lenient literals,
//! Integer/Long/Double selection).

use indexmap::IndexMap;
use std::fmt;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct JSONException(pub String);
impl fmt::Display for JSONException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "org.json.JSONException: {}", self.0)
    }
}
impl std::error::Error for JSONException {}
pub type JResult<T> = Result<T, JSONException>;

/// A value stored in a JSONObject/JSONArray. `Null` is `JSONObject.NULL`;
/// an absent value (Java `null`) is `Option::None` at the API level.
#[derive(Clone, Debug)]
pub enum JV {
    Null,
    Bool(bool),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Str(String),
    Obj(JSONObject),
    Arr(JSONArray),
}

impl From<bool> for JV { fn from(v: bool) -> Self { JV::Bool(v) } }
impl From<i32> for JV { fn from(v: i32) -> Self { JV::Int(v) } }
impl From<i64> for JV { fn from(v: i64) -> Self { JV::Long(v) } }
/// `put(String, double)` widening a Java float argument: callers that passed
/// a `float` to `put(name, double)` must use `JV::Double(f as f64)`; a `Float`
/// object (autoboxed `put(name, (Object) f)`) is `JV::Float`.
impl From<f64> for JV { fn from(v: f64) -> Self { JV::Double(v) } }
impl From<&str> for JV { fn from(v: &str) -> Self { JV::Str(v.to_string()) } }
impl From<String> for JV { fn from(v: String) -> Self { JV::Str(v) } }
impl From<&String> for JV { fn from(v: &String) -> Self { JV::Str(v.clone()) } }
impl From<JSONObject> for JV { fn from(v: JSONObject) -> Self { JV::Obj(v) } }
impl From<&JSONObject> for JV { fn from(v: &JSONObject) -> Self { JV::Obj(v.clone()) } }
impl From<JSONArray> for JV { fn from(v: JSONArray) -> Self { JV::Arr(v) } }
impl From<&JSONArray> for JV { fn from(v: &JSONArray) -> Self { JV::Arr(v.clone()) } }

fn is_number(v: &JV) -> bool {
    matches!(v, JV::Int(_) | JV::Long(_) | JV::Float(_) | JV::Double(_))
}
fn double_value(v: &JV) -> Option<f64> {
    Some(match v {
        JV::Int(i) => *i as f64,
        JV::Long(l) => *l as f64,
        JV::Float(f) => *f as f64,
        JV::Double(d) => *d,
        _ => return None,
    })
}
/// Java `Number.longValue()` (saturating for floating values).
fn long_value(v: &JV) -> i64 {
    match v {
        JV::Int(i) => *i as i64,
        JV::Long(l) => *l,
        JV::Float(f) => java_d2l(*f as f64),
        JV::Double(d) => java_d2l(*d),
        _ => 0,
    }
}
/// Java `Number.intValue()`: `(int)` of the value (Long truncates bits).
fn int_value(v: &JV) -> i32 {
    match v {
        JV::Int(i) => *i,
        JV::Long(l) => *l as i32,
        JV::Float(f) => java_d2i(*f as f64),
        JV::Double(d) => java_d2i(*d),
        _ => 0,
    }
}
pub fn java_d2l(d: f64) -> i64 {
    if d.is_nan() { 0 } else { d as i64 }
}
pub fn java_d2i(d: f64) -> i32 {
    if d.is_nan() { 0 } else { d as i32 }
}

fn check_double(d: f64) -> JResult<f64> {
    if d.is_infinite() || d.is_nan() {
        return Err(JSONException(format!("Forbidden numeric value: {}", notemaker_core::jfmt::double(d))));
    }
    Ok(d)
}

/// `JSONObject.numberToString`.
pub fn number_to_string(v: &JV) -> JResult<String> {
    let d = double_value(v).ok_or_else(|| JSONException("Number must be non-null".into()))?;
    check_double(d)?;
    if let JV::Double(x) = v {
        if *x == 0.0 && x.is_sign_negative() {
            return Ok("-0".into());
        }
    }
    let l = long_value(v);
    if d == l as f64 {
        return Ok(l.to_string());
    }
    Ok(java_number_to_string(v))
}
/// `Number.toString()` / `String.valueOf(number)`.
pub fn java_number_to_string(v: &JV) -> String {
    match v {
        JV::Int(i) => i.to_string(),
        JV::Long(l) => l.to_string(),
        JV::Float(f) => notemaker_core::jfmt::float(*f),
        JV::Double(d) => notemaker_core::jfmt::double(*d),
        _ => String::new(),
    }
}

/// `JSON.toString(Object)`: strings as-is, other values via `String.valueOf`.
pub fn value_to_java_string(v: &JV) -> String {
    match v {
        JV::Str(s) => s.clone(),
        JV::Null => "null".into(),
        JV::Bool(b) => b.to_string(),
        JV::Obj(o) => o.to_string(),
        JV::Arr(a) => a.to_string(),
        n => java_number_to_string(n),
    }
}
fn to_double(v: &JV) -> Option<f64> {
    match v {
        JV::Str(s) => parse_java_double(s),
        n => double_value(n),
    }
}
fn to_int(v: &JV) -> Option<i32> {
    match v {
        JV::Int(i) => Some(*i),
        JV::Str(s) => parse_java_double(s).map(java_d2i),
        n if is_number(n) => Some(int_value(n)),
        _ => None,
    }
}
fn to_long(v: &JV) -> Option<i64> {
    match v {
        JV::Long(l) => Some(*l),
        JV::Str(s) => parse_java_double(s).map(java_d2l),
        n if is_number(n) => Some(long_value(n)),
        _ => None,
    }
}
fn to_bool(v: &JV) -> Option<bool> {
    match v {
        JV::Bool(b) => Some(*b),
        JV::Str(s) if s.eq_ignore_ascii_case("true") => Some(true),
        JV::Str(s) if s.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}
/// `Double.parseDouble` for the formats Java accepts (trimmed, optional
/// `d`/`f` suffix, `NaN`, `Infinity`).
pub fn parse_java_double(s: &str) -> Option<f64> {
    let t = s.trim_matches(|c: char| c <= ' ');
    let t = t.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(t);
    match t {
        "NaN" | "+NaN" | "-NaN" => return Some(f64::NAN),
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    if t.is_empty() || t.contains(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))) {
        return None;
    }
    t.parse::<f64>().ok()
}

fn type_name(v: &JV) -> &'static str {
    match v {
        JV::Null => "org.json.JSONObject$1",
        JV::Bool(_) => "java.lang.Boolean",
        JV::Int(_) => "java.lang.Integer",
        JV::Long(_) => "java.lang.Long",
        JV::Float(_) => "java.lang.Float",
        JV::Double(_) => "java.lang.Double",
        JV::Str(_) => "java.lang.String",
        JV::Obj(_) => "org.json.JSONObject",
        JV::Arr(_) => "org.json.JSONArray",
    }
}
fn type_mismatch_name(name: &str, v: &JV, required: &str) -> JSONException {
    JSONException(format!("Value {} at {} of type {} cannot be converted to {}", value_to_java_string(v), name, type_name(v), required))
}
fn type_mismatch_index(index: usize, v: &JV, required: &str) -> JSONException {
    JSONException(format!("Value {} at {} of type {} cannot be converted to {}", value_to_java_string(v), index, type_name(v), required))
}

fn checked(v: JV) -> JResult<JV> {
    if let Some(d) = double_value(&v) {
        check_double(d)?;
    }
    Ok(v)
}

// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct JSONObject(Arc<Mutex<IndexMap<String, JV>>>);

impl fmt::Debug for JSONObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string())
    }
}
/// Java `JSONObject.equals` is identity.
impl PartialEq for JSONObject {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl JSONObject {
    pub fn new() -> Self {
        Self::default()
    }
    /// `new JSONObject(String)`.
    pub fn parse(json: &str) -> JResult<Self> {
        match Tokener::new(json).next_value()? {
            JV::Obj(o) => Ok(o),
            v => Err(JSONException(format!("Value {} of type {} cannot be converted to JSONObject", value_to_java_string(&v), type_name(&v)))),
        }
    }
    /// A deep copy (`new JSONObject(other.toString())` style callers should use `parse`).
    pub fn length(&self) -> usize {
        self.0.lock().unwrap().len()
    }
    /// `put(name, value)`; `None` removes the mapping like `put(name, null)`.
    pub fn put(&self, name: &str, value: impl Into<JV>) -> JResult<&Self> {
        let v = checked(value.into())?;
        self.0.lock().unwrap().insert(name.to_string(), v);
        Ok(self)
    }
    pub fn put_opt(&self, name: &str, value: Option<JV>) -> JResult<&Self> {
        match value {
            None => {
                self.0.lock().unwrap().shift_remove(name);
                Ok(self)
            }
            Some(v) => self.put(name, v),
        }
    }
    /// `accumulate(name, value)`.
    pub fn accumulate(&self, name: &str, value: impl Into<JV>) -> JResult<&Self> {
        let v = checked(value.into())?;
        let mut m = self.0.lock().unwrap();
        match m.get(name).cloned() {
            None => {
                m.insert(name.into(), v);
            }
            Some(JV::Arr(a)) => {
                drop(m);
                a.put(v)?;
            }
            Some(cur) => {
                let a = JSONArray::new();
                a.put(cur)?;
                a.put(v)?;
                m.insert(name.into(), JV::Arr(a));
            }
        }
        Ok(self)
    }
    pub fn remove(&self, name: &str) -> Option<JV> {
        self.0.lock().unwrap().shift_remove(name)
    }
    pub fn has(&self, name: &str) -> bool {
        self.0.lock().unwrap().contains_key(name)
    }
    /// `isNull(name)`: absent or `JSONObject.NULL`.
    pub fn is_null(&self, name: &str) -> bool {
        matches!(self.opt(name), None | Some(JV::Null))
    }
    pub fn opt(&self, name: &str) -> Option<JV> {
        self.0.lock().unwrap().get(name).cloned()
    }
    pub fn get(&self, name: &str) -> JResult<JV> {
        self.opt(name).ok_or_else(|| JSONException(format!("No value for {name}")))
    }
    pub fn keys(&self) -> Vec<String> {
        self.0.lock().unwrap().keys().cloned().collect()
    }
    pub fn names(&self) -> Option<JSONArray> {
        let k = self.keys();
        if k.is_empty() {
            None
        } else {
            let a = JSONArray::new();
            for n in k {
                a.0.lock().unwrap().push(JV::Str(n));
            }
            Some(a)
        }
    }
    pub fn get_boolean(&self, name: &str) -> JResult<bool> {
        let v = self.get(name)?;
        to_bool(&v).ok_or_else(|| type_mismatch_name(name, &v, "boolean"))
    }
    pub fn opt_boolean(&self, name: &str) -> bool {
        self.opt_boolean_or(name, false)
    }
    pub fn opt_boolean_or(&self, name: &str, fallback: bool) -> bool {
        self.opt(name).as_ref().and_then(to_bool).unwrap_or(fallback)
    }
    pub fn get_double(&self, name: &str) -> JResult<f64> {
        let v = self.get(name)?;
        to_double(&v).ok_or_else(|| type_mismatch_name(name, &v, "double"))
    }
    pub fn opt_double(&self, name: &str) -> f64 {
        self.opt_double_or(name, f64::NAN)
    }
    pub fn opt_double_or(&self, name: &str, fallback: f64) -> f64 {
        self.opt(name).as_ref().and_then(to_double).unwrap_or(fallback)
    }
    pub fn get_int(&self, name: &str) -> JResult<i32> {
        let v = self.get(name)?;
        to_int(&v).ok_or_else(|| type_mismatch_name(name, &v, "int"))
    }
    pub fn opt_int(&self, name: &str) -> i32 {
        self.opt_int_or(name, 0)
    }
    pub fn opt_int_or(&self, name: &str, fallback: i32) -> i32 {
        self.opt(name).as_ref().and_then(to_int).unwrap_or(fallback)
    }
    pub fn get_long(&self, name: &str) -> JResult<i64> {
        let v = self.get(name)?;
        to_long(&v).ok_or_else(|| type_mismatch_name(name, &v, "long"))
    }
    pub fn opt_long(&self, name: &str) -> i64 {
        self.opt_long_or(name, 0)
    }
    pub fn opt_long_or(&self, name: &str, fallback: i64) -> i64 {
        self.opt(name).as_ref().and_then(to_long).unwrap_or(fallback)
    }
    pub fn get_string(&self, name: &str) -> JResult<String> {
        Ok(value_to_java_string(&self.get(name)?))
    }
    pub fn opt_string(&self, name: &str) -> String {
        self.opt_string_or(name, "")
    }
    pub fn opt_string_or(&self, name: &str, fallback: &str) -> String {
        self.opt(name).map(|v| value_to_java_string(&v)).unwrap_or_else(|| fallback.to_string())
    }
    pub fn get_json_object(&self, name: &str) -> JResult<JSONObject> {
        match self.get(name)? {
            JV::Obj(o) => Ok(o),
            v => Err(type_mismatch_name(name, &v, "JSONObject")),
        }
    }
    pub fn opt_json_object(&self, name: &str) -> Option<JSONObject> {
        match self.opt(name) {
            Some(JV::Obj(o)) => Some(o),
            _ => None,
        }
    }
    pub fn get_json_array(&self, name: &str) -> JResult<JSONArray> {
        match self.get(name)? {
            JV::Arr(a) => Ok(a),
            v => Err(type_mismatch_name(name, &v, "JSONArray")),
        }
    }
    pub fn opt_json_array(&self, name: &str) -> Option<JSONArray> {
        match self.opt(name) {
            Some(JV::Arr(a)) => Some(a),
            _ => None,
        }
    }
    /// `toString(indentSpaces)`.
    pub fn to_string_indent(&self, indent: usize) -> String {
        let mut out = String::new();
        write_value(&mut out, &JV::Obj(self.clone()), indent, 0).expect("finite");
        out
    }
    /// `new JSONObject(map)` via `JSONObject.wrap` from a core JSON value.
    pub fn from_value(v: &serde_json::Value) -> Option<JSONObject> {
        match wrap(v) {
            JV::Obj(o) => Some(o),
            _ => None,
        }
    }
    /// Converts to a core JSON value (numbers keep Long vs Double form).
    pub fn to_value(&self) -> serde_json::Value {
        unwrap(&JV::Obj(self.clone()))
    }
}
impl fmt::Display for JSONObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        // Java's toString() returns null on a JSONException; a non-finite
        // number cannot be stored, so failure is unreachable here.
        write_value(&mut out, &JV::Obj(self.clone()), 0, 0).map_err(|_| fmt::Error)?;
        f.write_str(&out)
    }
}

#[derive(Clone, Default)]
pub struct JSONArray(Arc<Mutex<Vec<JV>>>);
impl fmt::Debug for JSONArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string())
    }
}
impl PartialEq for JSONArray {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl JSONArray {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn parse(json: &str) -> JResult<Self> {
        match Tokener::new(json).next_value()? {
            JV::Arr(a) => Ok(a),
            v => Err(JSONException(format!("Value {} of type {} cannot be converted to JSONArray", value_to_java_string(&v), type_name(&v)))),
        }
    }
    pub fn length(&self) -> usize {
        self.0.lock().unwrap().len()
    }
    pub fn put(&self, value: impl Into<JV>) -> JResult<&Self> {
        let v = checked(value.into())?;
        self.0.lock().unwrap().push(v);
        Ok(self)
    }
    /// `put(index, value)`, padding with Java nulls (`JV::Null` placeholders
    /// print as `null`, as Java's null elements do).
    pub fn put_at(&self, index: usize, value: impl Into<JV>) -> JResult<&Self> {
        let v = checked(value.into())?;
        let mut a = self.0.lock().unwrap();
        while a.len() <= index {
            a.push(JV::Null);
        }
        a[index] = v;
        Ok(self)
    }
    pub fn remove(&self, index: usize) -> Option<JV> {
        let mut a = self.0.lock().unwrap();
        if index < a.len() {
            Some(a.remove(index))
        } else {
            None
        }
    }
    pub fn opt(&self, index: usize) -> Option<JV> {
        self.0.lock().unwrap().get(index).cloned()
    }
    pub fn get(&self, index: usize) -> JResult<JV> {
        let a = self.0.lock().unwrap();
        a.get(index).cloned().ok_or_else(|| JSONException(format!("Index {} out of range [0..{})", index, a.len())))
    }
    pub fn values(&self) -> Vec<JV> {
        self.0.lock().unwrap().clone()
    }
    pub fn get_double(&self, index: usize) -> JResult<f64> {
        let v = self.get(index)?;
        to_double(&v).ok_or_else(|| type_mismatch_index(index, &v, "double"))
    }
    pub fn opt_double(&self, index: usize) -> f64 {
        self.opt_double_or(index, f64::NAN)
    }
    pub fn opt_double_or(&self, index: usize, fallback: f64) -> f64 {
        self.opt(index).as_ref().and_then(to_double).unwrap_or(fallback)
    }
    pub fn get_int(&self, index: usize) -> JResult<i32> {
        let v = self.get(index)?;
        to_int(&v).ok_or_else(|| type_mismatch_index(index, &v, "int"))
    }
    pub fn opt_int(&self, index: usize) -> i32 {
        self.opt(index).as_ref().and_then(to_int).unwrap_or(0)
    }
    pub fn get_long(&self, index: usize) -> JResult<i64> {
        let v = self.get(index)?;
        to_long(&v).ok_or_else(|| type_mismatch_index(index, &v, "long"))
    }
    pub fn opt_long(&self, index: usize) -> i64 {
        self.opt(index).as_ref().and_then(to_long).unwrap_or(0)
    }
    pub fn get_boolean(&self, index: usize) -> JResult<bool> {
        let v = self.get(index)?;
        to_bool(&v).ok_or_else(|| type_mismatch_index(index, &v, "boolean"))
    }
    pub fn opt_boolean(&self, index: usize) -> bool {
        self.opt(index).as_ref().and_then(to_bool).unwrap_or(false)
    }
    pub fn get_string(&self, index: usize) -> JResult<String> {
        Ok(value_to_java_string(&self.get(index)?))
    }
    pub fn opt_string(&self, index: usize) -> String {
        self.opt(index).map(|v| value_to_java_string(&v)).unwrap_or_default()
    }
    pub fn get_json_object(&self, index: usize) -> JResult<JSONObject> {
        match self.get(index)? {
            JV::Obj(o) => Ok(o),
            v => Err(type_mismatch_index(index, &v, "JSONObject")),
        }
    }
    pub fn opt_json_object(&self, index: usize) -> Option<JSONObject> {
        match self.opt(index) {
            Some(JV::Obj(o)) => Some(o),
            _ => None,
        }
    }
    pub fn get_json_array(&self, index: usize) -> JResult<JSONArray> {
        match self.get(index)? {
            JV::Arr(a) => Ok(a),
            v => Err(type_mismatch_index(index, &v, "JSONArray")),
        }
    }
    pub fn opt_json_array(&self, index: usize) -> Option<JSONArray> {
        match self.opt(index) {
            Some(JV::Arr(a)) => Some(a),
            _ => None,
        }
    }
    pub fn to_string_indent(&self, indent: usize) -> String {
        let mut out = String::new();
        write_value(&mut out, &JV::Arr(self.clone()), indent, 0).expect("finite");
        out
    }
    pub fn from_value(v: &serde_json::Value) -> Option<JSONArray> {
        match wrap(v) {
            JV::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn to_value(&self) -> serde_json::Value {
        unwrap(&JV::Arr(self.clone()))
    }
}
impl fmt::Display for JSONArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        write_value(&mut out, &JV::Arr(self.clone()), 0, 0).map_err(|_| fmt::Error)?;
        f.write_str(&out)
    }
}

/// `JSONObject.quote(String)`.
pub fn quote(s: &str) -> String {
    let mut out = String::new();
    write_string(&mut out, s);
    out
}
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' | '/' => {
                out.push('\\');
                out.push(c);
            }
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) <= 0x1f => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
fn newline(out: &mut String, indent: usize, depth: usize) {
    if indent == 0 {
        return;
    }
    out.push('\n');
    for _ in 0..indent * depth {
        out.push(' ');
    }
}
fn write_value(out: &mut String, v: &JV, indent: usize, depth: usize) -> JResult<()> {
    match v {
        JV::Null => out.push_str("null"),
        JV::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JV::Str(s) => write_string(out, s),
        JV::Obj(o) => {
            let entries: Vec<(String, JV)> = o.0.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                write_string(out, k);
                out.push(':');
                if indent > 0 {
                    out.push(' ');
                }
                write_value(out, v, indent, depth + 1)?;
            }
            if !entries.is_empty() {
                newline(out, indent, depth);
            }
            out.push('}');
        }
        JV::Arr(a) => {
            let values = a.values();
            out.push('[');
            for (i, v) in values.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, indent, depth + 1);
                write_value(out, v, indent, depth + 1)?;
            }
            if !values.is_empty() {
                newline(out, indent, depth);
            }
            out.push(']');
        }
        n => out.push_str(&number_to_string(n)?),
    }
    Ok(())
}

/// `JSONObject.wrap` of a core JSON value (`new JSONObject(map)`).
pub fn wrap(v: &serde_json::Value) -> JV {
    use serde_json::Value as V;
    match v {
        V::Null => JV::Null,
        V::Bool(b) => JV::Bool(*b),
        V::Number(n) => {
            if let Some(i) = n.as_i64() {
                if n.is_i64() || n.is_u64() {
                    return JV::Long(i);
                }
            }
            JV::Double(n.as_f64().unwrap_or(f64::NAN))
        }
        V::String(s) => JV::Str(s.clone()),
        V::Array(a) => {
            let out = JSONArray::new();
            out.0.lock().unwrap().extend(a.iter().map(wrap));
            JV::Arr(out)
        }
        V::Object(m) => {
            let out = JSONObject::new();
            {
                let mut o = out.0.lock().unwrap();
                for (k, v) in m {
                    o.insert(k.clone(), wrap(v));
                }
            }
            JV::Obj(out)
        }
    }
}
/// Converts an org.json value to the core JSON model (Int/Long → i64,
/// Float/Double → f64 with Java's printed digits preserved for floats).
pub fn unwrap(v: &JV) -> serde_json::Value {
    use serde_json::Value as V;
    match v {
        JV::Null => V::Null,
        JV::Bool(b) => V::Bool(*b),
        JV::Int(i) => V::from(*i as i64),
        JV::Long(l) => V::from(*l),
        JV::Float(f) => notemaker_core::json::from_float(*f),
        JV::Double(d) => serde_json::Number::from_f64(*d).map(V::Number).unwrap_or(V::Null),
        JV::Str(s) => V::String(s.clone()),
        JV::Arr(a) => V::Array(a.values().iter().map(unwrap).collect()),
        JV::Obj(o) => {
            let entries: Vec<(String, JV)> = o.0.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            let mut m = serde_json::Map::new();
            for (k, v) in entries {
                m.insert(k, unwrap(&v));
            }
            V::Object(m)
        }
    }
}

// ---------------------------------------------------------------------------
// JSONTokener

struct Tokener {
    s: Vec<char>,
    pos: usize,
}
impl Tokener {
    fn new(s: &str) -> Self {
        // Android's JSONTokener drops a leading byte-order mark.
        let s = s.strip_prefix('\u{feff}').unwrap_or(s);
        Tokener { s: s.chars().collect(), pos: 0 }
    }
    fn syntax(&self, message: &str) -> JSONException {
        let text: String = self.s.iter().collect();
        JSONException(format!("{} at character {} of {}", message, self.pos, text))
    }
    fn next_clean(&mut self) -> JResult<Option<char>> {
        while self.pos < self.s.len() {
            let c = self.s[self.pos];
            self.pos += 1;
            match c {
                '\t' | ' ' | '\n' | '\r' => continue,
                '/' => {
                    if self.pos == self.s.len() {
                        return Ok(Some(c));
                    }
                    match self.s[self.pos] {
                        '*' => {
                            self.pos += 1;
                            let rest: String = self.s[self.pos..].iter().collect();
                            match rest.find("*/") {
                                None => return Err(self.syntax("Unterminated comment")),
                                Some(i) => self.pos += rest[..i].chars().count() + 2,
                            }
                        }
                        '/' => {
                            self.pos += 1;
                            self.skip_to_end_of_line();
                        }
                        _ => return Ok(Some(c)),
                    }
                }
                '#' => self.skip_to_end_of_line(),
                _ => return Ok(Some(c)),
            }
        }
        Ok(None)
    }
    fn skip_to_end_of_line(&mut self) {
        while self.pos < self.s.len() {
            let c = self.s[self.pos];
            if c == '\r' || c == '\n' {
                self.pos += 1;
                break;
            }
            self.pos += 1;
        }
    }
    fn next_value(&mut self) -> JResult<JV> {
        let c = self.next_clean()?;
        match c {
            None => Err(self.syntax("End of input")),
            Some('{') => self.read_object(),
            Some('[') => self.read_array(),
            Some(q @ ('\'' | '"')) => Ok(JV::Str(self.next_string(q)?)),
            Some(_) => {
                self.pos -= 1;
                self.read_literal()
            }
        }
    }
    fn next_string(&mut self, quote: char) -> JResult<String> {
        let mut out = String::new();
        while self.pos < self.s.len() {
            let c = self.s[self.pos];
            self.pos += 1;
            if c == quote {
                return Ok(out);
            }
            if c == '\\' {
                if self.pos == self.s.len() {
                    return Err(self.syntax("Unterminated escape sequence"));
                }
                let e = self.s[self.pos];
                self.pos += 1;
                match e {
                    'u' => {
                        if self.pos + 4 > self.s.len() {
                            return Err(self.syntax("Unterminated escape sequence"));
                        }
                        let hex: String = self.s[self.pos..self.pos + 4].iter().collect();
                        self.pos += 4;
                        let unit = u16::from_str_radix(&hex, 16).map_err(|_| self.syntax(&format!("Invalid escape sequence: {hex}")))?;
                        // Combine UTF-16 surrogate pairs as Java strings would.
                        if (0xd800..0xdc00).contains(&unit) && self.s.get(self.pos) == Some(&'\\') && self.s.get(self.pos + 1) == Some(&'u') && self.pos + 6 <= self.s.len() {
                            let hex2: String = self.s[self.pos + 2..self.pos + 6].iter().collect();
                            if let Ok(low) = u16::from_str_radix(&hex2, 16) {
                                if (0xdc00..0xe000).contains(&low) {
                                    self.pos += 6;
                                    out.push_str(&String::from_utf16_lossy(&[unit, low]));
                                    continue;
                                }
                            }
                        }
                        out.push_str(&String::from_utf16_lossy(&[unit]));
                    }
                    't' => out.push('\t'),
                    'b' => out.push('\u{8}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    'f' => out.push('\u{c}'),
                    other => out.push(other),
                }
            } else {
                out.push(c);
            }
        }
        Err(self.syntax("Unterminated string"))
    }
    fn read_literal(&mut self) -> JResult<JV> {
        let start = self.pos;
        while self.pos < self.s.len() && !"{}[]/\\:,=;# \t\u{c}\r\n".contains(self.s[self.pos]) {
            self.pos += 1;
        }
        let literal: String = self.s[start..self.pos].iter().collect();
        if literal.is_empty() {
            return Err(self.syntax("Expected literal value"));
        }
        if literal.eq_ignore_ascii_case("null") {
            return Ok(JV::Null);
        }
        if literal.eq_ignore_ascii_case("true") {
            return Ok(JV::Bool(true));
        }
        if literal.eq_ignore_ascii_case("false") {
            return Ok(JV::Bool(false));
        }
        if !literal.contains('.') {
            let (base, digits) = if literal.starts_with("0x") || literal.starts_with("0X") {
                (16, &literal[2..])
            } else if literal.starts_with('0') && literal.len() > 1 {
                (8, &literal[1..])
            } else {
                (10, literal.as_str())
            };
            if let Ok(l) = i64::from_str_radix(digits, base) {
                if digits.chars().all(|c| c.is_digit(base) || (base == 10 && (c == '-' || c == '+'))) {
                    return Ok(if l <= i32::MAX as i64 && l >= i32::MIN as i64 { JV::Int(l as i32) } else { JV::Long(l) });
                }
            }
        }
        if let Some(d) = parse_java_double(&literal) {
            return Ok(JV::Double(d));
        }
        Ok(JV::Str(literal))
    }
    fn read_object(&mut self) -> JResult<JV> {
        let result = JSONObject::new();
        let first = self.next_clean()?;
        if first == Some('}') {
            return Ok(JV::Obj(result));
        } else if first.is_some() {
            self.pos -= 1;
        }
        loop {
            let name = self.next_value()?;
            let name = match name {
                JV::Str(s) => s,
                JV::Null => return Err(self.syntax("Names cannot be null")),
                other => return Err(self.syntax(&format!("Names must be strings, but {} is of type {}", value_to_java_string(&other), type_name(&other)))),
            };
            let sep = self.next_clean()?;
            if sep != Some(':') && sep != Some('=') {
                return Err(self.syntax(&format!("Expected ':' after {name}")));
            }
            if self.pos < self.s.len() && self.s[self.pos] == '>' {
                self.pos += 1;
            }
            let v = self.next_value()?;
            result.put(&name, v)?;
            match self.next_clean()? {
                Some('}') => return Ok(JV::Obj(result)),
                Some(';') | Some(',') => continue,
                _ => return Err(self.syntax("Unterminated object")),
            }
        }
    }
    fn read_array(&mut self) -> JResult<JV> {
        let result = JSONArray::new();
        let mut has_trailing_separator = false;
        loop {
            match self.next_clean()? {
                None => return Err(self.syntax("Unterminated array")),
                Some(']') => {
                    if has_trailing_separator {
                        result.0.lock().unwrap().push(JV::Null);
                    }
                    return Ok(JV::Arr(result));
                }
                Some(',') | Some(';') => {
                    result.0.lock().unwrap().push(JV::Null);
                    has_trailing_separator = true;
                    continue;
                }
                Some(_) => self.pos -= 1,
            }
            let v = self.next_value()?;
            result.0.lock().unwrap().push(v);
            match self.next_clean()? {
                Some(']') => return Ok(JV::Arr(result)),
                Some(',') | Some(';') => {
                    has_trailing_separator = true;
                    continue;
                }
                _ => return Err(self.syntax("Unterminated array")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formatting() {
        let o = JSONObject::new();
        o.put("a", 5.0).unwrap();
        o.put("b", 0.5).unwrap();
        o.put("c", "x/\"y\n").unwrap();
        o.put("d", -0.0).unwrap();
        o.put("e", JV::Float(0.1)).unwrap();
        o.put("f", 0.1f32 as f64).unwrap();
        assert_eq!(o.to_string(), r#"{"a":5,"b":0.5,"c":"x\/\"y\n","d":-0,"e":0.1,"f":0.10000000149011612}"#);
        let p = JSONObject::parse(r#"{"x":1,"y":3000000000,"z":1.5,"s":'q'}"#).unwrap();
        assert!(matches!(p.opt("x"), Some(JV::Int(1))));
        assert!(matches!(p.opt("y"), Some(JV::Long(3000000000))));
        assert_eq!(p.opt_string("z"), "1.5");
        assert_eq!(p.opt_string("s"), "q");
        assert_eq!(p.opt_int("z"), 1);
        let alias = p.clone();
        alias.put("x", 2).unwrap();
        assert_eq!(p.opt_int("x"), 2);
    }
}
