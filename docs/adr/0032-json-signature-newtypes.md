# ADR 0032: Thin newtypes for JSON in signatures

Status: accepted, 2026-11-03.

## Context

The `raw_type_in_signature` lint (ADR 0030) reported about 210 untyped
`serde_json::Map`/`Value`/`Vec<Value>` parameters and results. These are the
operations, replica states, entity content, action inputs and command results
that cross the journal, the network and the CLI's structured output. That JSON
must stay byte-compatible with the Java reference: `packages/core/tests/parity.sh`,
the committed parity snapshot and the CLI checks compare it exactly. It
depends on insertion order (`preserve_order`), on keeping unknown keys from
other replicas and old journals, on telling a missing key from `null`, and on
Java's number formatting.

Real serde structs for operations and entities would have to reproduce all of
that (`#[serde(flatten)]` for unknown keys, field order, `Option` vs `null`,
the Java number writer) in the hottest code: `Model::apply`, rendering and the
tile caches.

## Decision

- **Thin wrappers for shared JSON.** `notemaker_core::wire` defines three
  macros. `json_object_newtype!` wraps a `Map`. `json_array_newtype!` defines a
  borrowed `[Value]` type and an owned `Vec<Value>` type, like `str` and
  `String`. `json_value_newtype!` wraps any `Value`. Each wrapper holds the JSON
  unchanged and dereferences to it. Conversions go through trait impls, which
  ADR 0030 leaves unchecked: `From` for owned values, `TryFrom<Value>` (with
  `Json.map`'s "Expected JSON object" or `Json.list`'s "Expected JSON array"),
  and borrowed `From<&Map>`/`From<&[Value]>`/`TryFrom<&Value>` that return a
  reference to the wrapper without copying.
- The borrowed conversions are pointer casts. Every wrapper is
  `#[repr(transparent)]`, so the cast is sound. This is the only `unsafe` the
  wrappers add. `Entity` also converts an `Arc<Map>` into an
  `Arc<EntityContent>` the same way, so the allocation stays shared.
- **Core types** (`wire`): `Operation`, `Operations`/`OperationList`,
  `ReplicaState`, `ModelJson`, `EntityContent`, `EntityRef`,
  `EntityRefs`/`EntityRefList`, `EntityPage`, `ActionInput`,
  `Conflicts`/`ConflictList`.
- `Entity` is now a newtype over `Arc<EntityContent>` instead of the alias
  `Arc<Map>`. Identity is `Entity::ptr_eq`, and `make_mut` copies the content
  when it is shared. `store::State` dereferences to `ReplicaState`, which
  dereferences to `Map`.
- **One result type per command**, defined next to the command:
  - `store`: `CommitResult`, `HistoryResult`, `DeleteResult`, `AcceptResult`,
    `ReplicaPage`, `SyncResult`, `ServerInfo`, `ReplicaRootId`, plus
    `WithState<R>` in place of the `(Map, State)` tuples.
  - `journal`: `CheckpointResult`.
  - `collaboration`: `LockedEntityList`, `TransformSpec`.
  - `endpoint`: `EndpointDescriptor`, `RequestBody`, `ResponseBody`.
  - `shape_catalog`: `ShapeCatalogJson`.
  - CLI `local_proxy`: `CommandRequest`, `CommandInput`, `CommandReply`,
    `ExecutedCommand`, `SelectionRequest`/`SelectionReply`, `ReleaseReply`,
    `UiActionInput`/`UiReply`.
  - CLI `ui_hub`: `UiSessionList`, `UiHeartbeat`/`UiHeartbeatReply`,
    `UiCommandRequest`/`UiCommandReply`.
  - CLI `probe`: `ProbeRecord`.
  - Android `performance`: `ResourceSample`, `PerformanceReport`.
- **Real structs** for small, fixed shapes that are Rust-only and that the
  Java parity tests do not compare:
  - `store::EditorPreferences`: an ordered list of `EditorPreference` values.
    `TryFrom<&Map>` replaces `Store.validateSettings` and keeps its checks and
    messages. `From<&EditorPreferences> for Map` writes the keys in the order
    the input gave them.
  - Android `TileCacheMetrics` and `BoundsCacheMetrics`. The harnesses read
    fields instead of string keys, which replaces `long_metric(map, key)`.
  - Android `LatencyStats`, `AnimationState`/`ClipState` and `BrowserState`.
  - Each struct writes its JSON through `From<&T> for Value` with Java's keys
    and order.
- **Other signature changes:**
  - `Polygon::from_json`/`to_json` become `TryFrom<&Value>` and
    `From<&Polygon> for Value`.
  - The model's private field checks (`drawing`, `color`, `range`, `id`,
    `bool_`) are no longer public. Action inputs use `model::input_color`.
  - `Model::layer_items` returns `Arc<LayerItems>`.
  - `Store::commit_with_dependencies`/`commit_selected` take their
    dependencies as `&EntityRefs`, because every caller passes entity
    references.

## Consequences

- The JSON written to journals, the network and the CLI is unchanged. The
  wrappers neither copy nor reorder the data they hold.
- Signatures now say which JSON goes where. The wrappers do not check the
  JSON's shape beyond object/array/value. `Operation::kind()`-style typed
  views can be added later without changing the stored form.
- `EditorPreferences` is parsed before `appendWithSettings` runs, so a bad
  preference is now reported before an invalid actor. The proxy's
  `append settings -` still validates the operation before the preferences,
  as Java did.
- Call sites that build JSON use `.into()` or a wrapper literal such as
  `Operation(object! {…})`.
- Group G of the ADR 0030 rollout: the lint's untyped-JSON warnings outside
  `java_port` went to zero. The workspace total went from 224 to 51.
