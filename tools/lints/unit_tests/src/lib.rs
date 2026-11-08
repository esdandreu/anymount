#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_lint_defs;
extern crate rustc_session;
extern crate rustc_span;

use std::fs;
use std::path::Path;

use rustc_lint::{LateContext, LateLintPass, LintContext};
use syn::parse::Parser;
use syn::visit::{self, Visit};
use syn::{Attribute, Meta, Token, punctuated::Punctuated};

dylint_linting::dylint_library!();

rustc_lint_defs::declare_lint!(pub NO_TESTS_IN_DRIVER_MODULE, Deny, "driver module files must not contain tests");
rustc_lint_defs::declare_lint_pass!(NoTestsInDriverModule => [NO_TESTS_IN_DRIVER_MODULE]);

#[unsafe(no_mangle)]
pub fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[NO_TESTS_IN_DRIVER_MODULE]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(NoTestsInDriverModule)));
}

impl<'tcx> LateLintPass<'tcx> for NoTestsInDriverModule {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        if cx.tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).as_str() != "anymount" {
            return;
        }
        let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3) else {
            return;
        };
        for issue in check_driver_modules(&root.join("crates/anymount/src/drivers")) {
            cx.opt_span_lint(
                NO_TESTS_IN_DRIVER_MODULE,
                Some(rustc_span::DUMMY_SP),
                rustc_errors::DiagDecorator(|diagnostic| {
                    diagnostic.primary_message(issue);
                }),
            );
        }
    }
}

fn check_driver_modules(dir: &Path) -> Vec<String> {
    let mut issues = Vec::new();
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "rs")
                || !path.with_extension("").is_dir()
            {
                continue;
            }
            let source = fs::read_to_string(&path)?;
            if contains_tests(&source)? {
                issues.push(format!("{}: driver module files must not contain tests", path.display()));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        issues.push(format!("could not inspect driver modules in {}: {error}", dir.display()));
    }
    issues
}

fn contains_tests(source: &str) -> syn::Result<bool> {
    let syntax = syn::parse_file(source)?;
    let mut checker = TestChecker(false);
    checker.visit_file(&syntax);
    Ok(checker.0)
}

struct TestChecker(bool);

impl<'ast> Visit<'ast> for TestChecker {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if item.ident == "tests" {
            self.0 = true;
        }
        visit::visit_item_mod(self, item);
    }

    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        let path = attr.path();
        if path.segments.last().is_some_and(|segment| segment.ident == "test" || segment.ident == "wasm_bindgen_test")
            || (path.is_ident("cfg") && attr.parse_args::<Meta>().is_ok_and(|meta| has_test_predicate(&meta)))
        {
            self.0 = true;
        }
        visit::visit_attribute(self, attr);
    }
}

fn has_test_predicate(meta: &Meta) -> bool {
    match meta {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) => Punctuated::<Meta, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .is_ok_and(|items| items.iter().any(has_test_predicate)),
        Meta::NameValue(_) => false,
    }
}

#[test]
fn driver_module_test_detection() {
    for source in ["#[cfg(test)] mod checks {}", "mod tests;", "#[test] fn example() {}", "#[wasm_bindgen_test] fn example() {}", "#[cfg(all(test, unix))] fn example() {}", "#[tokio::test] async fn example() {}"] {
        assert!(contains_tests(source).unwrap(), "{source}");
    }
    for source in ["mod implementation; pub use implementation::Driver;", "#[cfg(unix)] mod native;", "fn testing() {}"] {
        assert!(!contains_tests(source).unwrap(), "{source}");
    }
}

