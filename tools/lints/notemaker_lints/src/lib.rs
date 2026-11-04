//! NoteMaker project lints. See `docs/adr/0030-newtype-signature-lint.md`.
#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;

use rustc_errors::DiagDecorator;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::{
    FnDecl, FnRetTy, ImplItem, ImplItemImplKind, ImplItemKind, Item, ItemKind, TraitItem,
    TraitItemKind,
};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty, TyCtxt};
use rustc_span::{Span, sym};
use serde::Deserialize;

dylint_linting::impl_late_lint! {
    /// ### What it does
    ///
    /// Flags parameter and return types of non-private functions (`pub`,
    /// `pub(crate)`, `pub(super)`, `pub(in ..)`) that are primitives, arrays,
    /// slices, tuples, or types defined outside the current crate.
    ///
    /// `Option<T>` and `Result<T, E>` are allowed when `T` and `E` pass, as
    /// are std ownership and concurrency wrappers (`Box`, `Arc`, `Rc`,
    /// `Weak`, `Mutex`, `RwLock`, `Cell`, `RefCell`, `OnceLock`, `Pin`,
    /// channels, join handles, ...). Atomics are allowed. `bool` is allowed
    /// in return types, so predicates need no wrapper.
    /// References, raw pointers, function pointers and generic arguments of
    /// local types are checked through to their inner types. Generic
    /// parameters, `impl Trait`, `dyn Trait`, `()` and `!` are allowed.
    /// Trait impl methods and the crate entry point are skipped, since their
    /// signatures are fixed elsewhere. Private functions (including private
    /// `#[no_mangle] extern "system"` JNI exports) are not checked.
    ///
    /// Types from crates listed in `allowed_crates` in the workspace
    /// `dylint.toml` count as local.
    ///
    /// ### Why is this bad?
    ///
    /// A `u32` or `String` says nothing about what the value means. A
    /// descriptive newtype documents the value and stops callers mixing up
    /// arguments of the same raw type.
    ///
    /// ### Example
    ///
    /// ```rust
    /// pub fn open(page: u32, title: String) {}
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// pub struct PageNumber(u32);
    /// pub struct DocumentTitle(String);
    /// pub fn open(page: PageNumber, title: DocumentTitle) {}
    /// ```
    pub RAW_TYPE_IN_SIGNATURE,
    Warn,
    "non-private function signature uses a primitive or foreign type instead of a descriptive newtype",
    RawTypeInSignature::new()
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    /// Crate names whose types are treated as local, e.g. `notemaker_core`.
    #[serde(default)]
    allowed_crates: Vec<String>,
}

struct RawTypeInSignature {
    config: Config,
}

impl RawTypeInSignature {
    fn new() -> Self {
        Self {
            config: dylint_linting::config_or_default(env!("CARGO_PKG_NAME")),
        }
    }

    fn check_signature<'tcx>(
        &self,
        cx: &LateContext<'tcx>,
        def_id: LocalDefId,
        decl: &'tcx FnDecl<'tcx>,
        span: Span,
        vis_span: Span,
    ) {
        let tcx = cx.tcx;
        if span.in_external_macro(cx.sess().source_map()) || is_private(cx, vis_span) {
            return;
        }
        if tcx
            .entry_fn(())
            .is_some_and(|(entry, _)| entry == def_id.to_def_id())
        {
            return;
        }
        let sig = tcx.fn_sig(def_id).instantiate_identity().skip_binder();
        for (hir_ty, ty) in decl.inputs.iter().zip(sig.inputs()) {
            self.report(cx, hir_ty.span, *ty, Position::Parameter);
        }
        if let FnRetTy::Return(hir_ty) = decl.output {
            self.report(cx, hir_ty.span, sig.output(), Position::Return);
        }
    }

    fn report<'tcx>(&self, cx: &LateContext<'tcx>, span: Span, ty: Ty<'tcx>, position: Position) {
        let mut found = Vec::new();
        self.collect(cx.tcx, ty, position, &mut found);
        found.dedup();
        for raw in found {
            cx.opt_span_lint(
                RAW_TYPE_IN_SIGNATURE,
                Some(span),
                DiagDecorator(|diag| {
                    diag.primary_message(format!(
                        "`{raw}` in the signature of a non-private function"
                    ));
                    diag.help("wrap it in a descriptive newtype defined in this crate");
                }),
            );
        }
    }

    /// Pushes every disallowed type reachable from `ty` onto `found`.
    fn collect<'tcx>(
        &self,
        tcx: TyCtxt<'tcx>,
        ty: Ty<'tcx>,
        position: Position,
        found: &mut Vec<Ty<'tcx>>,
    ) {
        match ty.kind() {
            ty::Bool if position == Position::Return => {}
            ty::Bool | ty::Char | ty::Int(_) | ty::Uint(_) | ty::Float(_) | ty::Str => {
                found.push(ty)
            }
            ty::Array(..) | ty::Slice(_) | ty::Pat(..) => found.push(ty),
            ty::Tuple(items) if !items.is_empty() => found.push(ty),
            ty::Ref(_, inner, _) | ty::RawPtr(inner, _) => {
                self.collect(tcx, *inner, position, found)
            }
            ty::FnPtr(sig_tys, _) => {
                let sig = sig_tys.skip_binder();
                for inner in sig.inputs() {
                    self.collect(tcx, *inner, Position::Parameter, found);
                }
                self.collect(tcx, sig.output(), Position::Return, found);
            }
            ty::Adt(def, args) => {
                let did = def.did();
                if is_std_leaf_allowed(tcx, did) {
                    return;
                }
                let transparent = tcx.is_diagnostic_item(sym::Option, did)
                    || tcx.is_diagnostic_item(sym::Result, did)
                    || def.is_box()
                    || is_std_wrapper(tcx, did);
                if !transparent && !self.is_local_crate(tcx, did.krate) {
                    found.push(ty);
                    return;
                }
                for inner in args.types() {
                    self.collect(tcx, inner, position, found);
                }
            }
            // Foreign `extern type`s.
            ty::Foreign(did) if !self.is_local_crate(tcx, did.krate) => found.push(ty),
            // `()`, `!`, generic parameters, `impl Trait`, `dyn Trait`,
            // projections and anything the compiler only uses internally.
            _ => {}
        }
    }

    fn is_local_crate(&self, tcx: TyCtxt<'_>, krate: rustc_hir::def_id::CrateNum) -> bool {
        krate == rustc_hir::def_id::LOCAL_CRATE
            || self
                .config
                .allowed_crates
                .iter()
                .any(|name| tcx.crate_name(krate).as_str() == name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Parameter,
    Return,
}

/// Ownership and concurrency wrappers from std whose contents are checked
/// instead of the wrapper itself.
const STD_WRAPPERS: &[&str] = &[
    "Arc",
    "Rc",
    "Weak",
    "Mutex",
    "MutexGuard",
    "RwLock",
    "RwLockReadGuard",
    "RwLockWriteGuard",
    "Cell",
    "RefCell",
    "Ref",
    "RefMut",
    "OnceCell",
    "OnceLock",
    "LazyCell",
    "LazyLock",
    "Pin",
    "Sender",
    "SyncSender",
    "Receiver",
    "JoinHandle",
    "ScopedJoinHandle",
    "Condvar",
    "Barrier",
];

fn is_std_crate(tcx: TyCtxt<'_>, did: rustc_hir::def_id::DefId) -> bool {
    matches!(tcx.crate_name(did.krate).as_str(), "std" | "core" | "alloc")
}

fn is_std_wrapper(tcx: TyCtxt<'_>, did: rustc_hir::def_id::DefId) -> bool {
    is_std_crate(tcx, did) && STD_WRAPPERS.contains(&tcx.item_name(did).as_str())
}

/// Atomics, and `Global`, the default allocator argument of `Box`/`Arc`/`Rc`.
fn is_std_leaf_allowed(tcx: TyCtxt<'_>, did: rustc_hir::def_id::DefId) -> bool {
    let name = tcx.item_name(did);
    is_std_crate(tcx, did) && (name.as_str().starts_with("Atomic") || name.as_str() == "Global")
}

/// Private means no visibility written, or `pub(self)`. This is syntactic on
/// purpose: rustc resolves `pub(crate)` at the crate root to the same
/// visibility as private, but the rule treats any written `pub` as non-private.
fn is_private(cx: &LateContext<'_>, vis_span: Span) -> bool {
    if vis_span.is_empty() {
        return true;
    }
    let written = cx
        .sess()
        .source_map()
        .span_to_snippet(vis_span)
        .unwrap_or_default();
    let written: String = written.split_whitespace().collect();
    written == "pub(self)" || written == "pub(inself)"
}

impl<'tcx> LateLintPass<'tcx> for RawTypeInSignature {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        if let ItemKind::Fn { sig, .. } = item.kind {
            self.check_signature(cx, item.owner_id.def_id, sig.decl, item.span, item.vis_span);
        }
    }

    fn check_impl_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx ImplItem<'tcx>) {
        // Trait impl methods have signatures fixed by the trait.
        if let ImplItemKind::Fn(sig, _) = item.kind
            && let ImplItemImplKind::Inherent { vis_span } = item.impl_kind
        {
            self.check_signature(cx, item.owner_id.def_id, sig.decl, item.span, vis_span);
        }
    }

    fn check_trait_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx TraitItem<'tcx>) {
        // Trait methods take the trait's visibility.
        if let TraitItemKind::Fn(sig, _) = item.kind {
            let trait_vis = cx
                .tcx
                .hir_expect_item(cx.tcx.local_parent(item.owner_id.def_id))
                .vis_span;
            self.check_signature(cx, item.owner_id.def_id, sig.decl, item.span, trait_vis);
        }
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
