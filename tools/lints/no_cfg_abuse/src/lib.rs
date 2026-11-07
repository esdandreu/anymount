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

rustc_lint_defs::declare_lint!(pub NO_CFG_ABUSE, Deny, "conditional compilation attributes are restricted");
rustc_lint_defs::declare_lint_pass!(NoCfgAbuse => [NO_CFG_ABUSE]);

#[unsafe(no_mangle)]
pub fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[NO_CFG_ABUSE]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(NoCfgAbuse)));
}

impl<'tcx> LateLintPass<'tcx> for NoCfgAbuse {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        if cx.tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).as_str()
            != "anymount"
        {
            return;
        }
        let Some(root) =
            Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3)
        else {
            return;
        };
        let mut issues = Vec::new();
        scan_dir(root, &root.join("crates"), &mut issues);
        for issue in issues {
            cx.opt_span_lint(
                NO_CFG_ABUSE,
                Some(rustc_span::DUMMY_SP),
                rustc_errors::DiagDecorator(|diagnostic| {
                    diagnostic.primary_message(issue);
                }),
            );
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
                issues.push(format!(
                    "could not read directory entry in {}: {error}",
                    dir.display()
                ));
                continue;
            }
        };
        let path = entry.path();
        if path.is_dir() {
            scan_dir(root, &path, issues);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            match fs::read_to_string(&path).map_err(|error| error.to_string()).and_then(|source| check_source(relative, &source).map_err(|error| error.to_string())) {
                Ok(count) if count > 0 => issues.push(format!("{}: {count} disallowed conditional compilation attribute(s)", relative.display())),
                Ok(_) => {}
                Err(error) => issues.push(format!("could not inspect {}: {error}", relative.display())),
            }
        }
    }
}

fn check_source(path: &Path, source: &str) -> syn::Result<usize> {
    let syntax = syn::parse_file(source)?;
    let mut checker = Checker { path, count: 0 };
    checker.visit_file(&syntax);
    Ok(checker.count)
}

struct Checker<'a> {
    path: &'a Path,
    count: usize,
}

impl<'ast> Visit<'ast> for Checker<'_> {
    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        if !allowed_attribute(self.path, &attr.meta) {
            self.count += 1;
        }
        visit::visit_attribute(self, attr);
    }
}

fn allowed_attribute(path: &Path, meta: &Meta) -> bool {
    let Meta::List(list) = meta else {
        return !meta.path().is_ident("cfg")
            && !meta.path().is_ident("cfg_attr");
    };
    if list.path.is_ident("cfg") {
        let Ok(predicate) = syn::parse2::<Meta>(list.tokens.clone()) else {
            return false;
        };
        if predicate.path().is_ident("test")
            && matches!(predicate, Meta::Path(_))
        {
            return true;
        }
        let path = path.to_string_lossy().replace('\\', "/");
        return match path.as_str() {
            "crates/anymount/src/lib.rs"
            | "crates/anymount/src/storages.rs" => is_not_wasm(&predicate),
            "crates/anymount/tests/browser_registry.rs" => is_wasm(&predicate),
            "crates/anymount/src/drivers.rs" => match &predicate {
                Meta::NameValue(value) if value.path.is_ident("target_os") => {
                    matches!(&value.value, syn::Expr::Lit(expr) if matches!(&expr.lit, syn::Lit::Str(s) if s.value() == "windows"))
                }
                Meta::NameValue(value) if value.path.is_ident("target_arch") => {
                    is_wasm(&predicate)
                }
                Meta::List(list) if list.path.is_ident("all") => {
                    let Ok(args) = Punctuated::<Meta, Token![,]>::parse_terminated
                        .parse2(list.tokens.clone()) else {
                        return false;
                    };
                    args.len() == 2
                        && matches!(&args[0], Meta::Path(path) if path.is_ident("unix"))
                        && matches!(&args[1], Meta::NameValue(value) if value.path.is_ident("feature") && matches!(&value.value, syn::Expr::Lit(expr) if matches!(&expr.lit, syn::Lit::Str(s) if s.value() == "fuse")))
                }
                _ => false,
            },
            "crates/anymount/src/storages/local.rs" => {
                matches!(&predicate, Meta::Path(p) if p.is_ident("unix") || p.is_ident("windows"))
            }
            "crates/anymount/src/config_dir/read_config.rs" => {
                matches!(&predicate, Meta::Path(p) if p.is_ident("unix"))
            }
            _ => false,
        };
    }
    if list.path.is_ident("cfg_attr") {
        let Ok(args) = Punctuated::<Meta, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
        else {
            return false;
        };
        return matches!(args.first(), Some(Meta::Path(p)) if p.is_ident("test"))
            && args.len() > 1
            && args.iter().skip(1).all(|meta| {
                !meta.path().is_ident("cfg")
                    && !meta.path().is_ident("cfg_attr")
            });
    }
    true
}

fn is_wasm(meta: &Meta) -> bool {
    matches!(meta, Meta::NameValue(value) if value.path.is_ident("target_arch") && matches!(&value.value, syn::Expr::Lit(expr) if matches!(&expr.lit, syn::Lit::Str(s) if s.value() == "wasm32")))
}

fn is_not_wasm(meta: &Meta) -> bool {
    matches!(meta, Meta::List(list) if list.path.is_ident("not") && syn::parse2::<Meta>(list.tokens.clone()).is_ok_and(|inner| is_wasm(&inner)))
}

#[test]
fn rejects_cfg_even_in_inactive_code() {
    let source = "#[cfg(not(target_arch = \"wasm32\"))] mod native; #[cfg(unix)] fn f() {}";
    assert_eq!(
        check_source(Path::new("crates/anymount/src/read_dir.rs"), source).unwrap(),
        2
    );
}

#[test]
fn accepts_only_explicit_exceptions() {
    assert_eq!(check_source(Path::new("crates/anymount/src/read_dir.rs"), "#[cfg(test)] mod tests {} #[cfg_attr(test, mockall::automock)] trait T {}").unwrap(), 0);
    assert_eq!(check_source(Path::new("crates/anymount/src/drivers.rs"), "#[cfg(target_os = \"windows\")] mod windows; #[cfg(feature = \"fuse\")] mod fuse; #[cfg(unix)] mod other;").unwrap(), 2);
    assert_eq!(check_source(Path::new("crates/anymount/src/drivers.rs"), "#[cfg(target_arch = \"wasm32\")] mod browser; #[cfg(all(unix, feature = \"fuse\"))] mod fuse; #[cfg(target_arch = \"arm\")] mod other;").unwrap(), 1);
    assert_eq!(check_source(Path::new("crates/anymount/src/storages.rs"), "#[cfg(not(target_arch = \"wasm32\"))] mod local; #[cfg(not(target_arch = \"arm\"))] mod other;").unwrap(), 1);
    assert_eq!(check_source(Path::new("crates/anymount/src/lib.rs"), "#[cfg(not(target_arch = \"wasm32\"))] mod config_dir; #[cfg(unix)] mod other;").unwrap(), 1);
    assert_eq!(check_source(Path::new("crates/anymount/tests/browser_registry.rs"), "#![cfg(target_arch = \"wasm32\")] #[cfg(unix)] fn other() {}").unwrap(), 1);
    assert_eq!(
        check_source(
            Path::new("crates/anymount/src/storages/local.rs"),
            "fn f() { #[cfg(unix)] {} #[cfg(windows)] {} }"
        )
        .unwrap(),
        0
    );
    assert_eq!(
        check_source(
            Path::new("crates/anymount/src/config_dir/read_config.rs"),
            "#[cfg(unix)] fn unix() {} #[cfg(windows)] fn windows() {}"
        )
        .unwrap(),
        1
    );
    assert_eq!(
        check_source(
            Path::new("other.rs"),
            "#[cfg_attr(test, cfg(unix))] fn f() {} #[cfg(not(test))] fn g() {}"
        )
        .unwrap(),
        2
    );
}
