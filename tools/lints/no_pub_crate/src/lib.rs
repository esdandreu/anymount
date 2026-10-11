#![feature(rustc_private)]
#![warn(unused_extern_crates)]

// A list of available compiler crates can be found here:
// https://doc.rust-lang.org/nightly/nightly-rustc/
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_lint_defs;
extern crate rustc_session;
extern crate rustc_span;

use rustc_hir::{FieldDef, ForeignItem, ImplItem, Item};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_span::Span;

dylint_linting::dylint_library!();

rustc_lint_defs::declare_lint!(
    pub NO_PUB_CRATE,
    Deny,
    "use module visibility instead of `pub(crate)`"
);

rustc_lint_defs::declare_lint_pass!(NoPubCrate => [NO_PUB_CRATE]);

#[unsafe(no_mangle)]
pub fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[NO_PUB_CRATE]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(NoPubCrate)));
}

impl<'tcx> LateLintPass<'tcx> for NoPubCrate {
    fn check_item(&mut self, cx: &LateContext<'tcx>, item: &'tcx Item<'tcx>) {
        check_visibility(cx, item.vis_span);
    }

    fn check_impl_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx ImplItem<'tcx>,
    ) {
        if let Some(span) = item.vis_span() {
            check_visibility(cx, span);
        }
    }

    fn check_foreign_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx ForeignItem<'tcx>,
    ) {
        check_visibility(cx, item.vis_span);
    }

    fn check_field_def(
        &mut self,
        cx: &LateContext<'tcx>,
        field: &'tcx FieldDef<'tcx>,
    ) {
        check_visibility(cx, field.vis_span);
    }
}

fn check_visibility(cx: &LateContext<'_>, span: Span) {
    let is_pub_crate =
        cx.tcx.sess.source_map().span_to_snippet(span).is_ok_and(
            |visibility| {
                visibility
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
                    == "pub(crate)"
            },
        );

    if is_pub_crate {
        cx.opt_span_lint(
            NO_PUB_CRATE,
            Some(span),
            rustc_errors::DiagDecorator(|diagnostic| {
                diagnostic
                    .primary_message("`pub(crate)` visibility is forbidden");
            }),
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
