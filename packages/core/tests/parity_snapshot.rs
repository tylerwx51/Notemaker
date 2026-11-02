//! Compares the Rust port against Java output captured by
//! `tests/parity.sh --update-snapshot`, so parity is checked by `cargo test`
//! without a JDK. `expected/all.sha256` holds one SHA-256 per section of the
//! Java `Parity all` output; sections listed in `expected/text-sections.txt`
//! are stored as text and compared line by line, tolerating float differences
//! within 1e-9 relative error (HotSpot sin/cos intrinsics differ from libm by
//! 1 ulp in rare cases) and the documented differences in
//! `expected/known-differences.txt`. Run `tests/parity.sh` for full diffs.

#[path = "parity/driver.rs"]
mod driver;

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/parity")
}

fn sections(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(name) = line.strip_prefix("== ") {
            out.push((name.trim_end_matches('\n').to_string(), String::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push_str(line);
        }
    }
    out
}

fn number_tokens(s: &str) -> (Vec<String>, Vec<String>) {
    let mut text = Vec::new();
    let mut nums = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    let mut start = 0;
    while i < b.len() {
        let begins = b[i].is_ascii_digit() || (b[i] == b'-' && i + 1 < b.len() && b[i + 1].is_ascii_digit());
        if begins {
            text.push(s[start..i].to_string());
            let n0 = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' || b[i] == b'E' || (b[i] == b'-' && b[i - 1] == b'E')) {
                i += 1;
            }
            nums.push(s[n0..i].to_string());
            start = i;
        } else {
            i += 1;
        }
    }
    text.push(s[start..].to_string());
    (text, nums)
}

fn numerically_close(x: &str, y: &str) -> bool {
    let (ta, na) = number_tokens(x);
    let (tb, nb) = number_tokens(y);
    ta == tb
        && na.len() == nb.len()
        && na.iter().zip(nb.iter()).all(|(p, q)| {
            p == q
                || match (p.parse::<f64>(), q.parse::<f64>()) {
                    (Ok(u), Ok(v)) => (u - v).abs() <= 1e-9 * u.abs().max(v.abs()).max(1e-300),
                    _ => false,
                }
        })
}

fn known() -> Vec<String> {
    std::fs::read_to_string(root().join("expected/known-differences.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(str::to_string)
        .collect()
}

#[test]
fn fixtures_match_java_snapshot() {
    let digests = std::fs::read_to_string(root().join("expected/all.sha256")).expect("run tests/parity.sh --update-snapshot");
    let texts: BTreeMap<String, String> = sections(&std::fs::read_to_string(root().join("expected/text-sections.txt")).unwrap()).into_iter().collect();
    let actual = sections(&driver::run(&root().join("fixtures"), "all", &[]));
    let known = known();
    let expected: Vec<(String, String)> = digests
        .lines()
        .map(|l| {
            let (hash, name) = l.split_once(' ').unwrap();
            (name.to_string(), hash.to_string())
        })
        .collect();
    let mut problems = Vec::new();
    let (mut tolerated, mut known_hits) = (0, 0);
    assert_eq!(expected.len(), actual.len(), "section count differs");
    for ((name, hash), (actual_name, text)) in expected.iter().zip(actual.iter()) {
        assert_eq!(name, actual_name, "section order differs");
        if let Some(java) = texts.get(name) {
            let (j, r): (Vec<&str>, Vec<&str>) = (java.lines().collect(), text.lines().collect());
            if j.len() != r.len() {
                problems.push(format!("{}: {} vs {} lines", name, j.len(), r.len()));
                continue;
            }
            for (a, b) in j.iter().zip(r.iter()) {
                if a == b {
                    continue;
                }
                if numerically_close(a, b) {
                    tolerated += 1;
                } else if known.iter().any(|k| a.contains(k.as_str()) || format!("== {}", name).contains(k.as_str())) {
                    known_hits += 1;
                } else {
                    problems.push(format!("{}:\n  java: {}\n  rust: {}", name, a, b));
                }
            }
            continue;
        }
        let digest: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{:02x}", b)).collect();
        if &digest != hash {
            problems.push(format!("{}: output differs from Java (run packages/core/tests/parity.sh for a diff)", name));
        }
    }
    eprintln!("parity snapshot: {} sections, {} float-tolerated lines, {} known differences", expected.len(), tolerated, known_hits);
    assert!(problems.is_empty(), "parity differences:\n{}", problems.join("\n"));
}

#[test]
fn reads_java_written_state() {
    let state = root().join("fixtures/state-java");
    let expected = std::fs::read_to_string(root().join("expected/read.txt")).expect("run tests/parity.sh --update-snapshot");
    let docs: Vec<String> = std::fs::read_to_string(state.join("DOCUMENTS")).unwrap().lines().map(str::to_string).collect();
    // Read-only replica access; the committed fixture is not modified.
    let mut args = vec![state.to_string_lossy().into_owned()];
    args.extend(docs);
    let actual = driver::run(&root().join("fixtures"), "read", &args);
    let differences: Vec<String> = expected
        .lines()
        .zip(actual.lines())
        .filter(|(a, b)| a != b)
        .map(|(a, b)| format!("  java: {}\n  rust: {}", &a[..a.len().min(300)], &b[..b.len().min(300)]))
        .collect();
    assert_eq!(expected.lines().count(), actual.lines().count(), "line count differs");
    assert!(differences.is_empty(), "Rust read of Java-written state differs:\n{}", differences.join("\n"));
}
