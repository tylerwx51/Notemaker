//! Port of `LayerEditor.java`: layer-owned scope for candidate lookup and ID resolution.

use crate::coverage::CoverageFactory;
use crate::geometry::Polygon;
use crate::ids::{EntityId, EntityKind, LayerId, PageNumber};
use crate::draw_item::DrawItem;
use crate::error::{require, require_with, Result};
use crate::jmath;
use crate::json::{self, Map, Value};
use crate::model::{Entity, Model};
use indexmap::IndexMap;

/// `LayerEditor`.
pub struct LayerEditor {
    items: IndexMap<String, DrawItem>,
    pub layer: LayerId,
    pub page: PageNumber,
}

/// Which items a [`LayerEditor`] holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemFilter {
    ActiveOnly,
    IncludeInactive,
}

/// How many hits a selection keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitMode {
    /// Only the topmost hit item.
    Topmost,
    /// Every hit item, topmost first.
    All,
}

/// `LayerEditor.page(request)`: validated integer page (default 0).
pub fn page(request: &Map) -> Result<PageNumber> {
    let zero = Value::from(0);
    let raw = json::get_or_default(request, "page", &zero);
    require(raw.is_number(), "Invalid page")?;
    let n = json::as_double(Some(raw))?;
    require(n.is_finite() && n == jmath::rint(n) && (0.0..=10000.0).contains(&n), "Invalid page")?;
    Ok(PageNumber(jmath::d2i(n)))
}

fn editable(model: &Model, layer: &str, page: i32) -> Result<()> {
    require(model.layers.contains_key(layer) && model.layer_active.get(layer) != Some(&false), "Layer is not editable")?;
    require((0..=10000).contains(&page), "Invalid page")
}

fn page_of(o: &Map) -> Result<i32> {
    json::as_int(o.get("page"))
}

impl LayerEditor {
    /// `new LayerEditor(model, layer, page)`: active items only.
    pub fn new(model: &Model, layer: &LayerId, page: PageNumber) -> Result<LayerEditor> {
        LayerEditor::build(model, layer.as_ref(), page.0, false, true)
    }

    /// `new LayerEditor(model, layer, page, includeInactive)`.
    pub fn with_filter(model: &Model, layer: &LayerId, page: PageNumber, filter: ItemFilter) -> Result<LayerEditor> {
        LayerEditor::build(model, layer.as_ref(), page.0, filter == ItemFilter::IncludeInactive, true)
    }

    /// `LayerEditor.forErase(model, layer, page)`: scans the tables directly.
    pub fn for_erase(model: &Model, layer: &LayerId, page: PageNumber) -> Result<LayerEditor> {
        LayerEditor::build(model, layer.as_ref(), page.0, false, false)
    }

    fn build(model: &Model, layer: &str, page: i32, include_inactive: bool, indexed: bool) -> Result<LayerEditor> {
        editable(model, layer, page)?;
        let source: Vec<Entity> = if indexed {
            model.layer_items(&LayerId::from(layer), PageNumber(page)).as_ref().clone()
        } else {
            let mut scoped = Vec::new();
            for o in model.strokes.values().chain(model.objects.values()) {
                if o.get("layer").and_then(Value::as_str) == Some(layer) && page_of(o)? == page {
                    scoped.push(o.clone());
                }
            }
            scoped
        };
        let mut items = IndexMap::new();
        for o in source {
            let item = DrawItem::new(model, o)?;
            if include_inactive || item.active() {
                items.insert(format!("{}:{}", item.kind, item.id), item);
            }
        }
        Ok(LayerEditor { items, layer: LayerId::from(layer), page: PageNumber(page) })
    }

    /// `items()`.
    pub fn items(&self) -> impl Iterator<Item = &DrawItem> {
        self.items.values()
    }

    /// `resolve(kind, id)`.
    pub fn resolve(&self, kind: &EntityKind, id: &EntityId) -> Result<&DrawItem> {
        let item = self.items.get(&format!("{}:{}", kind, id));
        require_with(item.is_some(), || format!("Item is outside the editable layer/page: {}", id))?;
        Ok(item.unwrap())
    }

    /// `resolve(refs)`: every reference must be in scope.
    pub fn resolve_refs<'a>(&self, refs: &'a [Value]) -> Result<&'a [Value]> {
        for raw in refs {
            let r = json::map(raw)?;
            self.resolve(&EntityKind::from(json::string(r, "kind")?), &EntityId::from(json::string(r, "id")?))?;
        }
        Ok(refs)
    }

    /// `LayerEditor.requireReferences(model, layer, page, refs)`.
    pub fn require_references(model: &Model, layer: &LayerId, page: PageNumber, refs: &[Value]) -> Result<()> {
        let (layer, page) = (&**layer, page.0);
        editable(model, layer, page)?;
        for raw in refs {
            let r = json::map(raw)?;
            let kind = json::string(r, "kind")?;
            let id = json::string(r, "id")?;
            let item = match kind {
                "stroke" => model.strokes.get(id),
                "object" => model.objects.get(id),
                _ => None,
            };
            let active = (if kind == "stroke" { &model.active } else { &model.object_active }).get(id) != Some(&false);
            let ok = match item {
                Some(item) => active && item.get("layer").and_then(Value::as_str) == Some(layer) && page_of(item)? == page,
                None => false,
            };
            require_with(ok, || format!("Item is outside the editable layer/page: {}", id))?;
        }
        Ok(())
    }

    /// `select(region, topmost, factory)`: topmost first; `None` region selects any painted item.
    pub fn select(&self, region: Option<&Polygon>, hits: HitMode, factory: &dyn CoverageFactory) -> Result<Vec<Value>> {
        let topmost = hits == HitMode::Topmost;
        let mut out = Vec::new();
        for item in self.items.values().rev() {
            let hit = match region {
                None => item.has_coverage(factory)?,
                Some(r) => item.intersects(r, factory)?,
            };
            if hit {
                out.push(Value::Object(item.ref_()));
                if topmost {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Static `LayerEditor.select(model, layer, page, region, topmost, factory)`:
    /// tests topmost items first without preparing unreachable `DrawItem`s.
    pub fn select_in(model: &Model, layer: &LayerId, page: PageNumber, region: Option<&Polygon>, hits: HitMode, factory: &dyn CoverageFactory) -> Result<Vec<Value>> {
        let topmost = hits == HitMode::Topmost;
        editable(model, layer.as_ref(), page.0)?;
        let ordered = model.layer_items(layer, page);
        let mut out = Vec::new();
        for source in ordered.iter().rev() {
            let kind = json::string(source, "kind")?;
            let id = json::string(source, kind)?;
            if (if kind == "stroke" { &model.active } else { &model.object_active }).get(id) == Some(&false) {
                continue;
            }
            let item = DrawItem::new(model, source.clone())?;
            let hit = match region {
                None => item.has_coverage(factory)?,
                Some(r) => item.intersects(r, factory)?,
            };
            if hit {
                out.push(Value::Object(item.ref_()));
                if topmost {
                    break;
                }
            }
        }
        Ok(out)
    }
}
