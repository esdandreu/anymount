#![feature(rustc_private)]
#![warn(unused_extern_crates)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_lint_defs;
extern crate rustc_session;
extern crate rustc_span;

use rustc_lint::{LateContext, LateLintPass, LintContext};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

dylint_linting::dylint_library!();

rustc_lint_defs::declare_lint!(
    pub MODULE_NAMES,
    Deny,
    "module names and mock exports must follow project conventions"
);

rustc_lint_defs::declare_lint_pass!(ModuleNames => [MODULE_NAMES]);

#[unsafe(no_mangle)]
pub fn register_lints(
    _sess: &rustc_session::Session,
    lint_store: &mut rustc_lint::LintStore,
) {
    lint_store.register_lints(&[MODULE_NAMES]);
    lint_store.register_late_lint_pass(Box::new(|_| Box::new(ModuleNames)));
}

impl<'tcx> LateLintPass<'tcx> for ModuleNames {
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
        for source_dir in ["crates", "tools"] {
            match validate_module_names(&workspace_root.join(source_dir)) {
                Ok(issues) => {
                    for issue in issues {
                        cx.opt_span_lint(
                            MODULE_NAMES,
                            Some(rustc_span::DUMMY_SP),
                            rustc_errors::DiagDecorator(|diagnostic| {
                                diagnostic.primary_message(issue);
                            }),
                        );
                    }
                }
                Err(error) => {
                    cx.opt_span_lint(
                        MODULE_NAMES,
                        Some(rustc_span::DUMMY_SP),
                        rustc_errors::DiagDecorator(|diagnostic| {
                            diagnostic.primary_message(format!(
                                "could not inspect Rust modules in `{source_dir}`: {error}"
                            ));
                        }),
                    );
                }
            }
        }
    }
}

fn validate_module_names(source_dir: &Path) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();
    find_rust_files(source_dir, &mut files)?;
    files.sort();

    let mut issues = Vec::new();
    for file in files {
        let contents = fs::read_to_string(&file)?;
        let syntax = syn::parse_file(&contents).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error)
        })?;
        let public_items = syntax
            .items
            .iter()
            .filter_map(public_item_name)
            .collect::<Vec<_>>();

        if is_mock_module_file(&file) {
            issues.extend(validate_mock_exports(&file, &syntax)?);
        }

        if public_items.is_empty() || is_special_module_file(&file) {
            continue;
        }

        let expected = public_items
            .iter()
            .map(|name| format!("{}.rs", to_snake_case(name)))
            .collect::<Vec<_>>();
        let actual_file_name = file.file_name().unwrap_or_default().to_string_lossy();
        if !expected.iter().any(|name| name == &actual_file_name) {
            let exports = public_items.join("`, `");
            let expected = expected.join("` or `");
            let display_path = file.strip_prefix(source_dir).unwrap_or(&file);
            issues.push(format!(
                "module `{}` exports `{exports}`; rename it to `{expected}`",
                display_path.display()
            ));
        }
    }

    Ok(issues)
}

fn validate_mock_exports(
    file: &Path,
    syntax: &syn::File,
) -> std::io::Result<Vec<String>> {
    let mut issues = Vec::new();
    let mut modules = BTreeSet::new();
    let mut reexports = BTreeSet::new();

    for item in &syntax.items {
        match item {
            syn::Item::Mod(module) => {
                modules.insert(module.ident.to_string());
                if !matches!(&module.vis, syn::Visibility::Inherited) {
                    issues.push(format!(
                        "mock module `{}` must be private; re-export its public items with `pub use module::Item`",
                        module.ident
                    ));
                }
            }
            syn::Item::Use(item_use)
                if matches!(&item_use.vis, syn::Visibility::Public(_)) =>
            {
                collect_reexports(&item_use.tree, &mut Vec::new(), &mut reexports);
            }
            _ => {}
        }
    }

    let parent = file.parent().unwrap_or_else(|| Path::new("."));
    for module in modules {
        let child_file = parent.join(format!("{module}.rs"));
        if !child_file.is_file() {
            continue;
        }

        let contents = fs::read_to_string(&child_file)?;
        let child_syntax = syn::parse_file(&contents).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error)
        })?;
        for item in &child_syntax.items {
            let Some(name) = public_item_name(item) else {
                continue;
            };
            if !reexports.contains(&(module.clone(), name.clone())) {
                issues.push(format!(
                    "mock module `{module}` exports public item `{name}` without `pub use {module}::{name}`"
                ));
            }
        }
    }

    Ok(issues)
}

fn collect_reexports(
    tree: &syn::UseTree,
    path: &mut Vec<String>,
    reexports: &mut BTreeSet<(String, String)>,
) {
    match tree {
        syn::UseTree::Path(tree_path) => {
            path.push(tree_path.ident.to_string());
            collect_reexports(&tree_path.tree, path, reexports);
            path.pop();
        }
        syn::UseTree::Name(tree_name) if path.len() == 1 => {
            reexports.insert((path[0].clone(), tree_name.ident.to_string()));
        }
        syn::UseTree::Rename(tree_rename) if path.len() == 1 => {
            reexports.insert((path[0].clone(), tree_rename.ident.to_string()));
        }
        syn::UseTree::Group(tree_group) => {
            for item in &tree_group.items {
                collect_reexports(item, path, reexports);
            }
        }
        _ => {}
    }
}

fn find_rust_files(
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
            find_rust_files(&path, files)?;
        } else if file_type.is_file()
            && path.extension().is_some_and(|extension| extension == "rs")
        {
            files.push(path);
        }
    }

    Ok(())
}

fn is_special_module_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        ["build.rs", "lib.rs", "main.rs", "mod.rs"]
            .iter()
            .any(|special_name| name == *special_name)
    })
}

fn is_mock_module_file(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name == "mod.rs")
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "mocks")
}

fn public_item_name(item: &syn::Item) -> Option<String> {
    let (visibility, name) = match item {
        syn::Item::Struct(item) => (&item.vis, &item.ident),
        syn::Item::Enum(item) => (&item.vis, &item.ident),
        syn::Item::Union(item) => (&item.vis, &item.ident),
        syn::Item::Type(item) => (&item.vis, &item.ident),
        syn::Item::Trait(item) => (&item.vis, &item.ident),
        syn::Item::TraitAlias(item) => (&item.vis, &item.ident),
        syn::Item::Fn(item) => (&item.vis, &item.sig.ident),
        syn::Item::Const(item) => (&item.vis, &item.ident),
        syn::Item::Static(item) => (&item.vis, &item.ident),
        _ => return None,
    };

    matches!(visibility, syn::Visibility::Public(_)).then(|| name.to_string())
}

fn to_snake_case(name: &str) -> String {
    let characters = name.chars().collect::<Vec<_>>();
    let mut snake_case = String::new();

    for (index, character) in characters.iter().copied().enumerate() {
        if character.is_uppercase() {
            let previous_is_lowercase = index > 0
                && (characters[index - 1].is_lowercase()
                    || characters[index - 1].is_ascii_digit());
            let acronym_ends = index > 0
                && characters[index - 1].is_uppercase()
                && characters
                    .get(index + 1)
                    .is_some_and(|next| next.is_lowercase());

            if previous_is_lowercase || acronym_ends {
                snake_case.push('_');
            }
            snake_case.extend(character.to_lowercase());
        } else {
            snake_case.push(character);
        }
    }

    snake_case
}

#[test]
fn reports_module_name_that_does_not_match_public_item() {
    let temp_dir = tempfile::tempdir().expect("temporary directory");
    let module_dir = temp_dir.path().join("domain");
    fs::create_dir_all(&module_dir).expect("module directory");
    fs::write(module_dir.join("driver.rs"), "pub struct MockDriver;")
    .expect("write module");

    let issues = validate_module_names(temp_dir.path())
        .expect("validate module names");

    assert_eq!(
        issues,
        ["module `domain/driver.rs` exports `MockDriver`; rename it to `mock_driver.rs`"]
    );
}

#[test]
fn accepts_module_name_that_matches_public_type() {
    let temp_dir = tempfile::tempdir().expect("temporary directory");
    let module_dir = temp_dir.path().join("domain");
    fs::create_dir_all(&module_dir).expect("module directory");
    fs::write(
        module_dir.join("mock_storage_config.rs"),
        "pub struct MockStorageConfig;",
    )
    .expect("write module");

    let issues = validate_module_names(temp_dir.path())
        .expect("validate module names");

    assert!(issues.is_empty());
}

#[test]
fn accepts_module_name_that_matches_public_function() {
    let temp_dir = tempfile::tempdir().expect("temporary directory");
    fs::write(
        temp_dir.path().join("read_file.rs"),
        "pub fn read_file() {}",
    )
    .expect("write module");

    let issues = validate_module_names(temp_dir.path())
        .expect("validate module names");

    assert!(issues.is_empty());
}

#[test]
fn requires_private_mock_modules_and_public_item_reexports() {
    let temp_dir = tempfile::tempdir().expect("temporary directory");
    let mocks_dir = temp_dir.path().join("test_utils/mocks");
    fs::create_dir_all(&mocks_dir).expect("mocks directory");
    fs::write(mocks_dir.join("mod.rs"), "pub mod sample;")
        .expect("write mocks module");
    fs::write(mocks_dir.join("sample.rs"), "pub struct Sample;")
        .expect("write sample module");

    let issues = validate_module_names(temp_dir.path())
        .expect("validate module names");

    assert!(issues.iter().any(|issue| issue.contains(
        "mock module `sample` must be private"
    )));
    assert!(issues.iter().any(|issue| issue.contains(
        "without `pub use sample::Sample`"
    )));
}

#[test]
fn accepts_private_mock_modules_with_explicit_reexports() {
    let temp_dir = tempfile::tempdir().expect("temporary directory");
    let mocks_dir = temp_dir.path().join("test_utils/mocks");
    fs::create_dir_all(&mocks_dir).expect("mocks directory");
    fs::write(
        mocks_dir.join("mod.rs"),
        "mod sample;\npub use sample::Sample;",
    )
    .expect("write mocks module");
    fs::write(mocks_dir.join("sample.rs"), "pub struct Sample;")
        .expect("write sample module");

    let issues = validate_module_names(temp_dir.path())
        .expect("validate module names");

    assert!(issues.is_empty());
}

#[test]
fn converts_acronyms_to_snake_case() {
    assert_eq!(to_snake_case("HTTPStorage"), "http_storage");
    assert_eq!(to_snake_case("MockStorageConfig"), "mock_storage_config");
}
