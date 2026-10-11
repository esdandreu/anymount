#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_lint_defs;
extern crate rustc_session;
extern crate rustc_span;

use rustc_lint::{LateContext, LateLintPass, LintContext};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

dylint_linting::dylint_library!();

rustc_lint_defs::declare_lint!(
    pub DRIVER_README,
    Deny,
    "driver READMEs must document the Unmounted, Mounted, Connected, and Disconnected states"
);

rustc_lint_defs::declare_lint_pass!(DriverReadme => [DRIVER_README]);

#[unsafe(no_mangle)]
pub fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[DRIVER_README]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(DriverReadme)));
}

impl<'tcx> LateLintPass<'tcx> for DriverReadme {
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
        let drivers_dir = workspace_root.join("crates/anymount/src/drivers");

        match validate_driver_readmes(&drivers_dir) {
            Ok(issues) => {
                for issue in issues {
                    cx.opt_span_lint(
                        DRIVER_README,
                        Some(rustc_span::DUMMY_SP),
                        rustc_errors::DiagDecorator(|diagnostic| {
                            diagnostic.primary_message(issue);
                        }),
                    );
                }
            }
            Err(error) => {
                cx.opt_span_lint(
                    DRIVER_README,
                    Some(rustc_span::DUMMY_SP),
                    rustc_errors::DiagDecorator(|diagnostic| {
                        diagnostic.primary_message(format!(
                            "could not inspect driver documentation: {error}"
                        ));
                    }),
                );
            }
        }
    }
}

fn validate_driver_readmes(drivers_dir: &Path) -> std::io::Result<Vec<String>> {
    let mut driver_dirs = fs::read_dir(drivers_dir)?
        .map(|entry| {
            let entry = entry?;
            let file_type = entry.file_type()?;
            Ok((entry.path(), file_type))
        })
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|(path, file_type)| file_type.is_dir().then_some(path))
        .collect::<Vec<_>>();
    driver_dirs.sort();

    let mut issues = Vec::new();
    for driver_dir in driver_dirs {
        let readme_path = driver_dir.join("README.md");
        let display_path = readme_path
            .strip_prefix(drivers_dir)
            .unwrap_or(&readme_path);
        let display_path = display_path.display();

        let contents = match fs::read_to_string(&readme_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                issues.push(format!(
                    "driver `{display_path}` is missing its README.md"
                ));
                continue;
            }
            Err(error) => {
                issues.push(format!(
                    "could not read driver `{display_path}` README.md: {error}"
                ));
                continue;
            }
        };

        for state in missing_states(&contents) {
            issues.push(format!(
                "driver `{display_path}` README.md is missing the `{state}` state bullet under `## States`"
            ));
        }
    }

    Ok(issues)
}

fn missing_states(contents: &str) -> Vec<&'static str> {
    const REQUIRED_STATES: [&str; 4] = [
        "Unmounted",
        "Mounted",
        "Connected",
        "Disconnected",
    ];

    let mut in_states = false;
    let mut documented = HashSet::new();
    for line in contents.lines() {
        if line.starts_with("## ") {
            in_states = line.trim() == "## States";
            continue;
        }
        if !in_states {
            continue;
        }

        let bullet = line.trim_start();
        if let Some(label) = bullet
            .strip_prefix("- **")
            .or_else(|| bullet.strip_prefix("* **"))
            && let Some((state, _)) = label.split_once(":**")
            && REQUIRED_STATES.contains(&state)
        {
            documented.insert(state);
        }
    }

    REQUIRED_STATES
        .into_iter()
        .filter(|state| !documented.contains(state))
        .collect()
}

#[test]
fn requires_all_state_bullets_under_level_two_states_section() {
    let states = "## States\n\n- **Unmounted:** A\n- **Mounted:** B\n  - **Connected:** C\n  - **Disconnected:** D\n";
    assert!(missing_states(states).is_empty());
    assert_eq!(
        missing_states("### States\n- **Unmounted:** A\n- **Mounted:** B\n- **Connected:** C\n- **Disconnected:** D\n"),
        ["Unmounted", "Mounted", "Connected", "Disconnected"]
    );
}

#[test]
fn reports_missing_readme() {
    let temp_dir = tempfile::tempdir().expect("temporary directory");
    let drivers_dir = temp_dir.path().join("drivers");
    fs::create_dir_all(drivers_dir.join("example")).expect("driver directory");

    let issues =
        validate_driver_readmes(&drivers_dir).expect("validate readmes");

    assert_eq!(issues.len(), 1);
    assert!(issues[0].contains("example/README.md"));
    assert!(issues[0].contains("missing its README.md"));
}

#[test]
fn reports_missing_states() {
    let temp_dir = tempfile::tempdir().expect("temporary directory");
    let drivers_dir = temp_dir.path().join("drivers");
    let driver_dir = drivers_dir.join("example");
    fs::create_dir_all(&driver_dir).expect("driver directory");
    fs::write(driver_dir.join("README.md"), "# Example\n\n## States\n- **Unmounted:** A\n")
        .expect("write README");

    let issues =
        validate_driver_readmes(&drivers_dir).expect("validate readmes");

    assert_eq!(issues.len(), 3);
    assert!(issues.iter().any(|issue| issue.contains("`Mounted` state bullet")));
    assert!(issues.iter().any(|issue| issue.contains("`Connected` state bullet")));
    assert!(issues.iter().any(|issue| issue.contains("`Disconnected` state bullet")));
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
