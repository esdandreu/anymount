#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_lint_defs;
extern crate rustc_session;
extern crate rustc_span;

use rustc_hir::{AmbigArg, Expr, ExprKind, Pat, PatKind, QPath, Ty, TyKind};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_span::Span;

dylint_linting::dylint_library!();

rustc_lint_defs::declare_lint!(
    pub NO_SUPER_OUTSIDE_USE,
    Deny,
    "import `super` items in a `use` statement instead of using `super::` paths"
);

rustc_lint_defs::declare_lint_pass!(NoSuperOutsideUse => [NO_SUPER_OUTSIDE_USE]);

#[unsafe(no_mangle)]
pub fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[NO_SUPER_OUTSIDE_USE]);
    lint_store
        .register_late_lint_pass(Box::new(|_| Box::new(NoSuperOutsideUse)));
}

impl<'tcx> LateLintPass<'tcx> for NoSuperOutsideUse {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if matches!(expr.kind, ExprKind::Path(QPath::Resolved(..))) {
            check_super_span(cx, expr.span);
        }
    }

    fn check_ty(
        &mut self,
        cx: &LateContext<'tcx>,
        ty: &'tcx Ty<'tcx, AmbigArg>,
    ) {
        if matches!(ty.kind, TyKind::Path(QPath::Resolved(..))) {
            check_super_span(cx, ty.span);
        }
    }

    fn check_pat(&mut self, cx: &LateContext<'tcx>, pat: &'tcx Pat<'tcx>) {
        match pat.kind {
            PatKind::Struct(..) | PatKind::TupleStruct(..) => {
                check_super_span(cx, pat.span);
            }
            PatKind::Expr(pat_expr) => check_super_span(cx, pat_expr.span),
            _ => {}
        }
    }
}

fn check_super_span(cx: &LateContext<'_>, span: Span) {
    let is_super_path = cx
        .tcx
        .sess
        .source_map()
        .span_to_snippet(span)
        .is_ok_and(|path| path.trim_start().starts_with("super::"));

    if is_super_path {
        cx.opt_span_lint(
            NO_SUPER_OUTSIDE_USE,
            Some(span),
            rustc_errors::DiagDecorator(|diagnostic| {
                diagnostic.primary_message(
                    "import `super` items in a `use` statement instead of using `super::` paths",
                );
            }),
        );
    }
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
