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
use std::path::{Path, PathBuf};

dylint_linting::dylint_library!();

rustc_lint_defs::declare_lint!(
    pub NO_MOD_RS,
    Deny,
    "Rust source files must not be named `mod.rs`"
);

rustc_lint_defs::declare_lint_pass!(NoModRs => [NO_MOD_RS]);

#[unsafe(no_mangle)]
pub fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[NO_MOD_RS]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(NoModRs)));
}

impl<'tcx> LateLintPass<'tcx> for NoModRs {
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
        let mut files = Vec::new();

        for source_dir in ["crates", "tools"] {
            if let Err(error) =
                find_mod_rs_files(&workspace_root.join(source_dir), &mut files)
            {
                emit_issue(
                    cx,
                    format!("could not inspect `{source_dir}`: {error}"),
                );
                return;
            }
        }

        files.sort();
        for file in files {
            let display_path =
                file.strip_prefix(workspace_root).unwrap_or(&file).display();
            let issue = if let Some(module) = flat_module_name(&file) {
                format!(
                    "module `{module}` must use `{module}/mod.rs`, not `{display_path}`"
                )
            } else {
                format!("Rust source file `{display_path}` is named `mod.rs`")
            };
            emit_issue(cx, issue);
        }
    }
}

fn find_mod_rs_files(
    directory: &Path,
    files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            find_mod_rs_files(&path, files)?;
        } else if file_type.is_file() {
            if path.file_name().is_some_and(|name| name == "mod.rs")
                && !is_allowed_mod_rs(&path)
            {
                files.push(path);
            } else if flat_module_name(&path).is_some() {
                files.push(path);
            }
        }
    }

    Ok(())
}

fn is_allowed_mod_rs(path: &Path) -> bool {
    path.ancestors().any(|ancestor| {
        ancestor
            .file_name()
            .is_some_and(|name| name == "test_utils" || name == "mocks")
    })
}

fn flat_module_name(path: &Path) -> Option<&'static str> {
    match path.file_name()?.to_str()? {
        "test_utils.rs" => Some("test_utils"),
        "mocks.rs" => Some("mocks"),
        _ => None,
    }
}

fn emit_issue(cx: &LateContext<'_>, issue: String) {
    cx.opt_span_lint(
        NO_MOD_RS,
        Some(rustc_span::DUMMY_SP),
        rustc_errors::DiagDecorator(|diagnostic| {
            diagnostic.primary_message(issue);
        }),
    );
}
