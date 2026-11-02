//! Port of `ToolDisplay.java`: human-readable tool inputs; output remains
//! verbatim in the expandable agent row.

use crate::org_json::{value_to_java_string, JSONObject, JV};
use std::collections::HashSet;

/// `final class ToolDisplay`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolDisplay {
    pub summary: String,
    pub details: String,
}

/// Java regex `\s`: `[ \t\n\x0B\f\r]`.
fn java_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// `value.replaceAll("\\s+", " ").trim()`.
fn collapse(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_space = false;
    for c in value.chars() {
        if java_space(c) {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    // String.trim(): strips code units <= ' '.
    out.trim_matches(|c: char| c <= ' ').to_string()
}

/// `key.replace('_', ' ').replaceAll("([a-z])([A-Z])", "$1 $2")` with the
/// first character upper-cased.
fn label_for(key: &str) -> String {
    let chars: Vec<char> = key.chars().map(|c| if c == '_' { ' ' } else { c }).collect();
    let mut label = String::with_capacity(key.len() + 4);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_lowercase() && i + 1 < chars.len() && chars[i + 1].is_ascii_uppercase() {
            label.push(c);
            label.push(' ');
            label.push(chars[i + 1]);
            i += 2;
        } else {
            label.push(c);
            i += 1;
        }
    }
    let mut it = label.chars();
    match it.next() {
        None => label,
        Some(first) => {
            // Character.toUpperCase(char) maps one UTF-16 unit to one.
            let mut upper = first.to_uppercase();
            let mapped = match (upper.next(), upper.next()) {
                (Some(u), None) => u,
                _ => first,
            };
            let mut out = String::with_capacity(label.len());
            out.push(mapped);
            out.push_str(it.as_str());
            out
        }
    }
}

impl ToolDisplay {
    /// `ToolDisplay(String name, String raw)` (parses `raw` with org.json).
    pub fn new(name: &str, raw: &str) -> ToolDisplay {
        let args = match JSONObject::parse(raw) {
            Ok(args) => args,
            Err(_) => {
                return ToolDisplay { summary: name.to_string(), details: if raw.is_empty() { "No arguments".to_string() } else { raw.to_string() } };
            }
        };
        let path = args.opt_string_or("path", &args.opt_string_or("file_path", &args.opt_string_or("file", "")));
        let mut primary;
        let mut description = String::new();
        let mut used: HashSet<&str> = HashSet::new();
        match name {
            "read" | "write" | "edit" | "ls" => {
                primary = path.clone();
                description.push_str(if path.is_empty() { "." } else { &path });
                used.extend(["path", "file_path", "file"]);
            }
            "bash" | "powershell" => {
                primary = args.opt_string("command");
                description.push_str(&primary);
                used.insert("command");
            }
            "grep" | "find" => {
                primary = args.opt_string("pattern") + &(if path.is_empty() { String::new() } else { format!(" in {path}") });
                description.push_str("Pattern: ");
                description.push_str(&args.opt_string("pattern"));
                if !path.is_empty() {
                    description.push_str("\nIn: ");
                    description.push_str(&path);
                }
                used.extend(["pattern", "path", "file_path", "file"]);
            }
            "mock.inspect" => {
                primary = args.opt_string("document");
            }
            _ => {
                primary = path.clone();
            }
        }
        for key in args.keys() {
            if used.contains(key.as_str()) {
                continue;
            }
            let value = args.opt(&key);
            if !description.is_empty() {
                description.push_str("\n\n");
            }
            let label = match key.as_str() {
                "oldText" => "Replace".to_string(),
                "newText" => "With".to_string(),
                "content" => "Content".to_string(),
                "offset" => "Starting line".to_string(),
                "limit" => "Limit".to_string(),
                "timeout" => "Timeout (seconds)".to_string(),
                _ => label_for(&key),
            };
            let formatted = match &value {
                Some(JV::Obj(o)) => o.to_string_indent(2),
                Some(JV::Arr(a)) => a.to_string_indent(2),
                Some(v) => value_to_java_string(v),
                None => "null".to_string(),
            };
            description.push_str(&label);
            description.push_str(if formatted.contains('\n') || key == "content" || key == "oldText" || key == "newText" { ":\n" } else { ": " });
            description.push_str(&formatted);
        }
        primary = collapse(&primary);
        let units: Vec<u16> = primary.encode_utf16().collect();
        if units.len() > 160 {
            primary = String::from_utf16_lossy(&units[..157]) + "…";
        }
        let summary = format!("{}{}", name, if primary.is_empty() { String::new() } else { format!(" {primary}") });
        let details = if description.is_empty() { "No arguments".to_string() } else { description };
        ToolDisplay { summary, details }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unparsable_arguments() {
        assert_eq!(ToolDisplay::new("bash", ""), ToolDisplay { summary: "bash".into(), details: "No arguments".into() });
        assert_eq!(ToolDisplay::new("bash", "nope"), ToolDisplay { summary: "bash".into(), details: "nope".into() });
    }

    #[test]
    fn read_and_edit() {
        let d = ToolDisplay::new("edit", r#"{"path":"a/b.txt","oldText":"x","newText":"y","maxLines":3,"some_key":true}"#);
        assert_eq!(d.summary, "edit a/b.txt");
        assert_eq!(d.details, "a/b.txt\n\nReplace:\nx\n\nWith:\ny\n\nMax Lines: 3\n\nSome key: true");
        let d = ToolDisplay::new("read", "{}");
        assert_eq!(d.summary, "read");
        assert_eq!(d.details, ".");
    }

    #[test]
    fn grep_and_bash() {
        let d = ToolDisplay::new("grep", r#"{"pattern":"foo","path":"src"}"#);
        assert_eq!(d.summary, "grep foo in src");
        assert_eq!(d.details, "Pattern: foo\nIn: src");
        let d = ToolDisplay::new("bash", "{\"command\":\"ls\\n  -la\",\"timeout\":5}");
        assert_eq!(d.summary, "bash ls -la");
        assert_eq!(d.details, "ls\n  -la\n\nTimeout (seconds): 5");
        let d = ToolDisplay::new("other", "{}");
        assert_eq!(d, ToolDisplay { summary: "other".into(), details: "No arguments".into() });
    }

    #[test]
    fn nested_and_long() {
        let d = ToolDisplay::new("mock.inspect", r#"{"document":"notes","opts":{"a":1}}"#);
        assert_eq!(d.summary, "mock.inspect notes");
        assert_eq!(d.details, "Document: notes\n\nOpts:\n{\n  \"a\": 1\n}");
        let long = "x".repeat(200);
        let d = ToolDisplay::new("bash", &format!("{{\"command\":\"{long}\"}}"));
        assert_eq!(d.summary, format!("bash {}…", "x".repeat(157)));
    }
}
