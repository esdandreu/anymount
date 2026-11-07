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
    pub NO_EXTRA_CRATES,
    Deny,
    "only anymount, anymount-cli, and anymount-tui may exist in crates/"
);

rustc_lint_defs::declare_lint_pass!(NoExtraCrates => [NO_EXTRA_CRATES]);

#[unsafe(no_mangle)]
pub fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[NO_EXTRA_CRATES]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(NoExtraCrates)));
}

impl<'tcx> LateLintPass<'tcx> for NoExtraCrates {
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
        match extra_crate_directories(&workspace_root.join("crates")) {
            Ok(paths) => {
                for path in paths {
                    let display_path =
                        path.strip_prefix(workspace_root).unwrap_or(&path);
                    emit_issue(
                        cx,
                        format!(
                            "disallowed crate directory `{}`",
                            display_path.display()
                        ),
                    );
                }
            }
            Err(error) => {
                emit_issue(cx, format!("could not inspect `crates`: {error}"))
            }
        }
    }
}

fn extra_crate_directories(directory: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && !["anymount", "anymount-cli", "anymount-tui"]
                .iter()
                .any(|name| entry.file_name() == *name)
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn emit_issue(cx: &LateContext<'_>, issue: String) {
    cx.opt_span_lint(
        NO_EXTRA_CRATES,
        Some(rustc_span::DUMMY_SP),
        rustc_errors::DiagDecorator(|diagnostic| {
            diagnostic.primary_message(issue);
        }),
    );
}

#[test]
fn finds_only_extra_directories_in_sorted_order() {
    let directory = tempfile::tempdir().unwrap();
    for name in [
        "anymount",
        "anymount-cli",
        "anymount-tui",
        "z-extra",
        "a-extra",
    ] {
        fs::create_dir(directory.path().join(name)).unwrap();
    }
    fs::write(directory.path().join("not-a-directory"), "").unwrap();

    assert_eq!(
        extra_crate_directories(directory.path()).unwrap(),
        vec![
            directory.path().join("a-extra"),
            directory.path().join("z-extra")
        ]
    );
}

#[test]
fn reports_directory_read_errors() {
    let directory = tempfile::tempdir().unwrap();
    assert!(
        extra_crate_directories(&directory.path().join("missing")).is_err()
    );
}
