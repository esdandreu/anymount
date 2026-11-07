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
use syn::visit::{self, Visit};

dylint_linting::dylint_library!();

rustc_lint_defs::declare_lint!(pub SHARED_TEST_UTILS, Deny, "trait implementations in test modules must be shared test utilities");
rustc_lint_defs::declare_lint_pass!(SharedTestUtils => [SHARED_TEST_UTILS]);

#[unsafe(no_mangle)]
pub fn register_lints(_sess: &rustc_session::Session, store: &mut rustc_lint::LintStore) {
    store.register_lints(&[SHARED_TEST_UTILS]);
    store.register_late_lint_pass(Box::new(|_| Box::new(SharedTestUtils)));
}

impl<'tcx> LateLintPass<'tcx> for SharedTestUtils {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        if cx.tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).as_str() != "anymount" {
            return;
        }
        let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3) else {
            return;
        };
        let mut issues = Vec::new();
        scan_dir(root, &root.join("crates"), &mut issues);
        for issue in issues {
            cx.opt_span_lint(SHARED_TEST_UTILS, Some(rustc_span::DUMMY_SP), rustc_errors::DiagDecorator(|diagnostic| {
                diagnostic.primary_message(issue);
            }));
        }
    }
}

fn scan_dir(root: &Path, dir: &Path, issues: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            issues.push(format!("could not read {}: {error}", dir.display()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                issues.push(format!("could not read directory entry in {}: {error}", dir.display()));
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != "target" {
                scan_dir(root, &path, issues);
            }
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            match fs::read_to_string(&path).map_err(|error| error.to_string()).and_then(|source| count_impls(&source).map_err(|error| error.to_string())) {
                Ok(count) if count > 0 => issues.push(format!("{}: {count} trait implementation(s) in test modules; move them to shared test utilities", relative.display())),
                Ok(_) => {}
                Err(error) => issues.push(format!("could not inspect {}: {error}", relative.display())),
            }
        }
    }
}

fn count_impls(source: &str) -> syn::Result<usize> {
    let syntax = syn::parse_file(source)?;
    let mut checker = Checker { in_tests: false, count: 0, lines: source.lines().collect() };
    checker.visit_file(&syntax);
    Ok(checker.count)
}

struct Checker<'a> {
    in_tests: bool,
    count: usize,
    lines: Vec<&'a str>,
}

impl Checker<'_> {
    fn is_excepted(&self, item: &syn::ItemImpl) -> bool {
        let line = item.attrs.first().map_or_else(
            || item.impl_token.span.start().line,
            |attr| attr.pound_token.span.start().line,
        );
        line.checked_sub(2)
            .and_then(|index| self.lines.get(index))
            .and_then(|line| line.trim().strip_prefix("// private test util:"))
            .is_some_and(|reason| !reason.trim().is_empty())
    }
}

impl<'ast> Visit<'ast> for Checker<'_> {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if !self.in_tests && module.ident == "test_utils" {
            return;
        }
        let previous = self.in_tests;
        self.in_tests |= module.attrs.iter().any(|attr| {
            attr.path().is_ident("cfg")
                && attr.parse_args::<syn::Meta>().is_ok_and(|meta| meta.path().is_ident("test") && matches!(meta, syn::Meta::Path(_)))
        });
        visit::visit_item_mod(self, module);
        self.in_tests = previous;
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if self.in_tests && item.trait_.is_some() && !self.is_excepted(item) {
            self.count += 1;
        }
        visit::visit_item_impl(self, item);
    }
}

#[test]
fn detects_trait_impls_in_nested_test_modules_and_functions() {
    let source = r#"
        #[cfg(test)] mod tests {
            impl Trait for Fake {}
            mod nested { fn test() { impl Other for Fake {} } }
            impl Fake { fn method() {} }
        }
        impl Trait for Production {}
    "#;
    assert_eq!(count_impls(source).unwrap(), 2);
}

#[test]
fn allows_reasoned_comment_immediately_before_impl_or_attributes() {
    let source = r#"
        #[cfg(test)] mod tests {
            // private test util: Tests the domain contract.
            impl Trait for Fake {}
            // private test util: Tests serialization.
            #[some_attribute]
            impl Other for Fake {}
            // private test util:
            impl Third for Fake {}
            // private test util: Unrelated comment.

            impl Fourth for Fake {}
        }
    "#;
    assert_eq!(count_impls(source).unwrap(), 2);
}

#[test]
fn permits_shared_test_utils_and_other_modules() {
    let source = r#"
        #[cfg(test)] mod test_utils { impl Trait for Fake {} }
        mod production { impl Trait for Real {} }
        #[cfg(not(test))] mod other { impl Trait for Other {} }
        #[cfg(test)] mod tests { mod test_utils { impl Trait for Local {} } }
    "#;
    assert_eq!(count_impls(source).unwrap(), 1);
}
