//! `EditScope::Append` (ADR 0027): Store's action edits must produce the same
//! journal records and cached state as the Full path. Debug builds cross-check
//! every such edit against the Full path inside the journal (panicking on a
//! difference), so these scenarios exercise that check; the replay at the end
//! compares the cache with a fresh read of the files.

use notemaker_core::ids::{ActorId, DocumentName};
use notemaker_core::store::{self, EditorPreferences, HistoryDirection, State, Store};
use notemaker_core::{json, object, Model, Value};
use std::path::PathBuf;

fn directory(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("notemaker-append-scope-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn stroke(id: &str, points: usize) -> notemaker_core::Map {
    let points: Vec<Value> = (0..points).map(|i| Value::Array(vec![json::double(i as f64), json::double(i as f64 * 0.5), json::double(0.5)])).collect();
    object! {"kind" => "stroke", "stroke" => id, "layer" => "base", "page" => 0, "color" => -16777216i64, "width" => 4, "brush" => "pen", "points" => Value::Array(points)}
}

fn rect(id: &str, x: i64) -> Vec<Value> {
    vec![Value::Object(object! {"kind" => "object", "object" => id, "shape" => "rect", "layer" => "base", "page" => 0, "x" => x, "y" => 10, "w" => 40, "h" => 20, "color" => -16777216i64, "width" => 2, "fontSize" => 24, "text" => ""})]
}

/// The cached state equals a fresh replay of the base file and journal.
fn assert_replays(dir: &PathBuf, document: &str, cached: &State) {
    let copy = directory(&format!("{}-copy", document));
    std::fs::create_dir_all(&copy).unwrap();
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".json") || name.ends_with(".journal") {
            std::fs::copy(entry.path(), copy.join(&name)).unwrap();
        }
    }
    let fresh = Store::new(&copy, &DocumentName::from(document)).unwrap().read().unwrap();
    assert_eq!(json::write_map(&fresh), json::write_map(cached), "cache differs from replay");
    let _ = std::fs::remove_dir_all(&copy);
}

#[test]
fn action_edits_match_full_path_while_state_is_shared() {
    let dir = directory("shared");
    let store = Store::writer(&dir, &DocumentName::from("shared")).unwrap();
    store.append(&object! {"kind" => "document", "type" => "fixed"}.into(), &ActorId::from("alice")).unwrap();
    // Held reads share the cached map, so each edit has to unshare it first;
    // history entry identities must survive that.
    let mut held: Vec<State> = Vec::new();
    for i in 0..12 {
        held.push(store.read().unwrap());
        store.append_with_settings((&stroke(&format!("s{i}"), 20 + i)).into(), &ActorId::from("alice"), Some(&EditorPreferences::try_from(&object! {"undo" => Value::Array((0..=i).map(|n| Value::from(format!("s{n}"))).collect())}).unwrap())).unwrap();
        store.commit(&ActorId::from("alice"), &mut |_m: &mut Model| Ok((rect(&format!("o{i}"), i as i64 * 50)).into())).unwrap();
        if i % 3 == 2 {
            store.history(&ActorId::from("alice"), HistoryDirection::Undo).unwrap();
            store.history(&ActorId::from("alice"), HistoryDirection::Redo).unwrap();
            store.history(&ActorId::from("alice"), HistoryDirection::Undo).unwrap();
        }
        store.commit(&ActorId::from("bob"), &mut |_m: &mut Model| Ok((rect(&format!("b{i}"), 600)).into())).unwrap();
        if held.len() > 3 {
            held.remove(0);
        }
    }
    // A Full-scope edit between action edits.
    store.edit(&mut |s: &mut State| {
        s.insert("peerId".into(), Value::from("peer"));
        Ok(())
    })
    .unwrap();
    store.commit(&ActorId::from("alice"), &mut |_m: &mut Model| Ok((rect("after-full", 5)).into())).unwrap();
    let state = store.read().unwrap();
    let model = store::model(&state).unwrap();
    assert_eq!(model.strokes.len(), 12);
    assert_eq!(json::list_at(json::map_at(json::map_at(&state, "actors").unwrap(), "bob").unwrap(), "undo").unwrap().len(), 12);
    drop(held);
    assert_replays(&dir, "shared", &state);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rejected_action_edits_keep_the_cache() {
    let dir = directory("rejected");
    let store = Store::writer(&dir, &DocumentName::from("rejected")).unwrap();
    store.append(&object! {"kind" => "document", "type" => "fixed"}.into(), &ActorId::from("alice")).unwrap();
    store.commit(&ActorId::from("alice"), &mut |_m: &mut Model| Ok((rect("a", 10)).into())).unwrap();
    let before = json::write_map(&store.read().unwrap());
    // No redo, a build failure and an invalid action leave the replica as it was.
    assert!(store.history(&ActorId::from("alice"), HistoryDirection::Redo).is_err());
    assert!(store.history(&ActorId::from("carol"), HistoryDirection::Undo).is_err());
    assert!(store.commit(&ActorId::from("alice"), &mut |_m: &mut Model| Err(notemaker_core::Error::illegal_argument("no"))).is_err());
    assert!(store.commit(&ActorId::from("alice"), &mut |_m: &mut Model| Ok(Vec::new().into())).is_err());
    assert_eq!(before, json::write_map(&store.read().unwrap()));
    store.history(&ActorId::from("alice"), HistoryDirection::Undo).unwrap();
    store.history(&ActorId::from("alice"), HistoryDirection::Redo).unwrap();
    store.append((&stroke("late", 5)).into(), &ActorId::from("alice")).unwrap();
    let state = store.read().unwrap();
    assert_eq!(json::list_at(&state, "pending").unwrap().len(), 5);
    assert_replays(&dir, "rejected", &state);
    let _ = std::fs::remove_dir_all(&dir);
}
