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

rustc_lint_defs::declare_lint!(pub INTEGRATION_TESTS, Deny, "test directories are forbidden");
rustc_lint_defs::declare_lint_pass!(IntegrationTests => [INTEGRATION_TESTS]);

#[unsafe(no_mangle)]
pub fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[INTEGRATION_TESTS]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(IntegrationTests)));
}

impl<'tcx> LateLintPass<'tcx> for IntegrationTests {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        if cx.tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).as_str() != "anymount" {
            return;
        }
        let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(3) else {
            return;
        };
        for source in ["crates", "tools"] {
            let mut folders = Vec::new();
            match find_test_folders(&root.join(source), &mut folders) {
                Ok(()) => {
                    folders.sort();
                    for folder in folders {
                        let path = folder.strip_prefix(root).unwrap_or(&folder).display();
                        emit_issue(cx, format!("test directory `{path}` is forbidden; keep tests in source"));
                    }
                }
                Err(error) => emit_issue(cx, format!("could not inspect `{source}`: {error}")),
            }
        }
    }
}

fn find_test_folders(directory: &Path, folders: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        if entry.file_name() == "target" {
            continue;
        }
        if entry.file_name() == "test" || entry.file_name() == "tests" {
            folders.push(path.clone());
        }
        find_test_folders(&path, folders)?;
    }
    Ok(())
}

fn emit_issue(cx: &LateContext<'_>, issue: String) {
    cx.opt_span_lint(
        INTEGRATION_TESTS,
        Some(rustc_span::DUMMY_SP),
        rustc_errors::DiagDecorator(|diagnostic| {
            diagnostic.primary_message(issue);
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::find_test_folders;
    use std::fs;

    #[test]
    fn detects_nested_test_and_tests_directories() {
        let root = tempfile::tempdir().expect("temporary directory");
        let test = root.path().join("crate/src/test");
        let tests = root.path().join("crate/tests");
        fs::create_dir_all(&test).expect("test directory");
        fs::create_dir_all(&tests).expect("tests directory");
        fs::create_dir_all(root.path().join("crate/test_utils")).expect("test utilities");
        fs::create_dir_all(root.path().join("target/tests")).expect("build output");
        let mut found = Vec::new();
        find_test_folders(root.path(), &mut found).expect("scan directories");
        found.sort();
        let mut expected = vec![test, tests];
        expected.sort();
        assert_eq!(found, expected);
    }

    #[test]
    fn reports_io_errors() {
        let root = tempfile::tempdir().expect("temporary directory");
        assert!(find_test_folders(&root.path().join("missing"), &mut Vec::new()).is_err());
    }
}
