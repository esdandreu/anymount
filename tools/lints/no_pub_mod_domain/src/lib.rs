#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_lint_defs;
extern crate rustc_session;
extern crate rustc_span;

use rustc_lint::{LateContext, LateLintPass, LintContext};
use std::fs;
use std::path::Path;

dylint_linting::dylint_library!();

rustc_lint_defs::declare_lint!(
    pub NO_PUB_MOD_DOMAIN,
    Deny,
    "domain.rs must keep its modules private and re-export their public items"
);

rustc_lint_defs::declare_lint_pass!(NoPubModDomain => [NO_PUB_MOD_DOMAIN]);

#[unsafe(no_mangle)]
pub fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[NO_PUB_MOD_DOMAIN]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(NoPubModDomain)));
}

impl<'tcx> LateLintPass<'tcx> for NoPubModDomain {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        if cx.tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).as_str()
            != "anymount"
        {
            return;
        }

        let lint_crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let Some(workspace_root) = lint_crate_dir.ancestors().nth(3) else {
            return;
        };
        let domain_file = workspace_root.join("crates/anymount/src/domain.rs");
        let issues = match fs::read_to_string(&domain_file) {
            Ok(source) => match public_modules(&source) {
                Ok(modules) => modules
                    .into_iter()
                    .map(|module| {
                        format!(
                            "`domain.rs` declares public module `{module}`; keep the module private and re-export its public items"
                        )
                    })
                    .collect(),
                Err(error) => {
                    vec![format!("could not parse `domain.rs`: {error}")]
                }
            },
            Err(error) => vec![format!(
                "could not read `domain.rs`: {error}"
            )],
        };

        for issue in issues {
            cx.opt_span_lint(
                NO_PUB_MOD_DOMAIN,
                Some(rustc_span::DUMMY_SP),
                rustc_errors::DiagDecorator(|diagnostic| {
                    diagnostic.primary_message(issue);
                }),
            );
        }
    }
}

fn public_modules(source: &str) -> syn::Result<Vec<String>> {
    let syntax = syn::parse_file(source)?;
    Ok(syntax
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module)
                if matches!(module.vis, syn::Visibility::Public(_)) =>
            {
                Some(module.ident.to_string())
            }
            _ => None,
        })
        .collect())
}

#[test]
fn reports_public_modules() {
    assert_eq!(
        public_modules("pub mod config;\nmod private;\n")
            .expect("valid Rust source"),
        ["config"]
    );
}

#[test]
fn accepts_private_modules_and_reexports() {
    assert!(
        public_modules("mod config;\npub use config::Config;\n")
            .expect("valid Rust source")
            .is_empty()
    );
}
