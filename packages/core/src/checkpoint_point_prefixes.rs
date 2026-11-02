//! Port of `CheckpointPointPrefixes.java`: checkpoint-only prefix references
//! for successive snapshots of one stroke.

use crate::error::{require, Result};
use crate::json::{self, Map, Value};
use std::collections::HashMap;

const FORMAT: &str = "stroke-prefix-v1";
const FORMAT_KEY: &str = "checkpointPointFormat";
const COUNTS_KEY: &str = "checkpointPointPrefixes";

/// `encode(state)`: replace repeated point prefixes in `events`/`pending`.
pub fn encode(state: &mut Map) -> Result<()> {
    let mut encoder = Encoder { prior: HashMap::new(), counts: Vec::new(), compressed: false };
    let events = encoder.operations(json::list_at(state, "events")?)?;
    let pending = encoder.operations(json::list_at(state, "pending")?)?;
    if !encoder.compressed {
        return Ok(());
    }
    state.insert("events".into(), Value::Array(events));
    state.insert("pending".into(), Value::Array(pending));
    state.insert(FORMAT_KEY.into(), Value::String(FORMAT.into()));
    state.insert(COUNTS_KEY.into(), Value::Array(encoder.counts));
    Ok(())
}

/// `decode(state)`: expand prefix references written by [`encode`].
pub fn decode(state: &mut Map) -> Result<()> {
    let format = state.shift_remove(FORMAT_KEY);
    let format = match format {
        None | Some(Value::Null) => {
            return require(!state.contains_key(COUNTS_KEY), "Checkpoint point prefix counts without format");
        }
        Some(f) => f,
    };
    require(format.as_str() == Some(FORMAT), "Unknown checkpoint point format")?;
    let counts = state.shift_remove(COUNTS_KEY).unwrap_or(Value::Null);
    let counts = json::list(&counts)?.clone();
    let mut decoder = Decoder { prior: HashMap::new(), counts, index: 0 };
    let events = decoder.operations(json::list_at(state, "events")?)?;
    state.insert("events".into(), Value::Array(events));
    let pending = decoder.operations(json::list_at(state, "pending")?)?;
    state.insert("pending".into(), Value::Array(pending));
    require(decoder.index == decoder.counts.len(), "Checkpoint point prefix count mismatch")
}

struct Encoder {
    prior: HashMap<String, Vec<Value>>,
    counts: Vec<Value>,
    compressed: bool,
}

impl Encoder {
    fn operations(&mut self, source: &[Value]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(source.len());
        for raw in source {
            out.push(self.operation(json::map(raw)?)?);
        }
        Ok(out)
    }
    fn operation(&mut self, op: &Map) -> Result<Value> {
        let kind = op.get("kind").and_then(Value::as_str);
        if kind == Some("batch") {
            let mut out = op.clone();
            let actions = self.operations(json::list(op.get("actions").unwrap_or(&Value::Null))?)?;
            out.insert("actions".into(), Value::Array(actions));
            return Ok(Value::Object(out));
        }
        if kind != Some("stroke") {
            return Ok(Value::Object(op.clone()));
        }
        let id = json::string(op, "stroke")?.to_string();
        let points = json::list_at(op, "points")?;
        let mut prefix = 0;
        if let Some(previous) = self.prior.get(&id) {
            while prefix < points.len() && prefix < previous.len() && points[prefix] == previous[prefix] {
                prefix += 1;
            }
        }
        self.prior.insert(id, points.clone());
        if prefix < 8 {
            self.counts.push(Value::from(0));
            return Ok(Value::Object(op.clone()));
        }
        self.counts.push(Value::from(prefix as i64));
        self.compressed = true;
        let mut out = op.clone();
        out.insert("points".into(), Value::Array(points[prefix..].to_vec()));
        Ok(Value::Object(out))
    }
}

struct Decoder {
    prior: HashMap<String, Vec<Value>>,
    counts: Vec<Value>,
    index: usize,
}

impl Decoder {
    fn operations(&mut self, source: &[Value]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(source.len());
        for raw in source {
            out.push(self.operation(json::map(raw)?)?);
        }
        Ok(out)
    }
    fn operation(&mut self, op: &Map) -> Result<Value> {
        let kind = op.get("kind").and_then(Value::as_str);
        if kind == Some("batch") {
            let mut out = op.clone();
            let actions = self.operations(json::list(op.get("actions").unwrap_or(&Value::Null))?)?;
            out.insert("actions".into(), Value::Array(actions));
            return Ok(Value::Object(out));
        }
        if kind != Some("stroke") {
            return Ok(Value::Object(op.clone()));
        }
        require(self.index < self.counts.len(), "Checkpoint point prefix count missing")?;
        let raw = self.counts[self.index].clone();
        self.index += 1;
        require(raw.is_number(), "Invalid checkpoint point prefix")?;
        let prefix = json::as_int(Some(&raw))?;
        require(prefix >= 0 && json::as_double(Some(&raw))? == prefix as f64, "Invalid checkpoint point prefix")?;
        let prefix = prefix as usize;
        let id = json::string(op, "stroke")?.to_string();
        let tail = json::list_at(op, "points")?;
        let previous = self.prior.get(&id);
        require(prefix == 0 || matches!(previous, Some(p) if prefix <= p.len()), "Invalid checkpoint point prefix reference")?;
        if prefix == 0 {
            self.prior.insert(id, tail.clone());
            return Ok(Value::Object(op.clone()));
        }
        require(prefix + tail.len() <= 10000, "Checkpoint stroke has too many points")?;
        let mut points = Vec::with_capacity(prefix + tail.len());
        points.extend_from_slice(&previous.unwrap()[..prefix]);
        points.extend_from_slice(tail);
        let mut out = op.clone();
        out.insert("points".into(), Value::Array(points.clone()));
        self.prior.insert(id, points);
        Ok(Value::Object(out))
    }
}
