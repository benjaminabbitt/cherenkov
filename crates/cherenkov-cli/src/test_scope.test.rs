use super::*;
use std::path::PathBuf;

use crate::selection::{SelectArgs, Selection};

fn plan_in(id: &str, file: &str) -> MutantPlan {
    use cherenkov_core::ports::SyntaxPort;
    let syntax = cherenkov_syntax::TreeSitterSyntax::builtin().unwrap();
    let rust = cherenkov_core::syntax::LanguageId::new("rust");
    let profile = syntax
        .profiles()
        .into_iter()
        .find(|p| p.lang == rust)
        .unwrap();
    let parsed = syntax
        .analyze(
            &rust,
            Path::new("lib.rs"),
            Arc::from("pub fn f() -> bool { true }\n"),
        )
        .unwrap();
    let mut plan =
        cherenkov_core::plan::plan_file(&parsed, &profile, &cherenkov_core::operator::catalog())
            .plans
            .remove(0);
    plan.id = MutantId(id.into());
    plan.file = PathBuf::from(file);
    plan
}

fn packages() -> Vec<Package> {
    ["core", "util", "app"]
        .map(|n| Package {
            name: n.to_string(),
            dir: PathBuf::from(format!("/w/{n}")),
        })
        .to_vec()
}

fn named(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

fn ids(scope: &Scope, package: &str) -> Vec<String> {
    scope
        .judged_by(package)
        .iter()
        .map(|i| i.0.clone())
        .collect()
}

// cargo-mutants' precedence (options.rs): --test-workspace=true, then
// --test-package, then the configuration's test_workspace=true, then its
// test_package, else the mutated package's tests.

#[test]
fn test_without_any_setting_a_mutant_is_judged_by_its_own_package() {
    assert_eq!(
        TestPackages::resolve(None, &[], None, &[]),
        TestPackages::Mutated
    );
}

#[test]
fn test_test_workspace_true_on_the_command_line_beats_test_package_there() {
    assert_eq!(
        TestPackages::resolve(Some(true), &named(&["a"]), None, &[]),
        TestPackages::Workspace
    );
}

#[test]
fn test_test_package_on_the_command_line_beats_the_configuration_s_workspace() {
    assert_eq!(
        TestPackages::resolve(None, &named(&["a", "b"]), Some(true), &[]),
        TestPackages::Named(named(&["a", "b"]))
    );
}

#[test]
fn test_test_workspace_false_on_the_command_line_beats_the_configuration_s_true() {
    assert_eq!(
        TestPackages::resolve(Some(false), &[], Some(true), &[]),
        TestPackages::Mutated
    );
}

#[test]
fn test_test_workspace_false_on_the_command_line_leaves_the_configuration_s_packages() {
    assert_eq!(
        TestPackages::resolve(Some(false), &[], Some(true), &named(&["a"])),
        TestPackages::Named(named(&["a"]))
    );
}

#[test]
fn test_the_configuration_s_workspace_true_beats_its_packages() {
    assert_eq!(
        TestPackages::resolve(None, &[], Some(true), &named(&["a"])),
        TestPackages::Workspace
    );
    assert_eq!(
        TestPackages::resolve(None, &[], Some(false), &named(&["a"])),
        TestPackages::Named(named(&["a"]))
    );
}

#[test]
fn test_by_default_a_mutant_is_judged_by_its_package_alone_and_only_those_packages_are_built() {
    let plans = [
        plan_in("m1", "core/src/lib.rs"),
        plan_in("m2", "app/src/main.rs"),
    ];
    let scope = Scope::of(&TestPackages::Mutated, &plans, &packages(), Path::new("/w"));
    assert_eq!(scope.build(), Some(named(&["app", "core"])));
    assert_eq!(ids(&scope, "core"), ["m1"]);
    assert_eq!(ids(&scope, "app"), ["m2"]);
    assert!(ids(&scope, "util").is_empty(), "no mutant, no program");
    assert_eq!(scope.mutated(), named(&["app", "core"]));
}

#[test]
fn test_named_test_packages_judge_every_mutant_and_alone_are_built() {
    let plans = [
        plan_in("m1", "core/src/lib.rs"),
        plan_in("m2", "app/src/main.rs"),
    ];
    let tests = TestPackages::Named(named(&["util"]));
    let scope = Scope::of(&tests, &plans, &packages(), Path::new("/w"));
    assert_eq!(scope.build(), Some(named(&["util"])));
    assert_eq!(ids(&scope, "util"), ["m1", "m2"]);
    assert!(ids(&scope, "core").is_empty());
    assert_eq!(
        scope.mutated(),
        named(&["app", "core"]),
        "the check is of what is mutated"
    );
}

#[test]
fn test_the_workspace_scope_builds_everything_and_every_program_judges_every_mutant() {
    let plans = [plan_in("m1", "core/src/lib.rs")];
    let scope = Scope::of(
        &TestPackages::Workspace,
        &plans,
        &packages(),
        Path::new("/w"),
    );
    assert_eq!(scope.build(), None);
    assert!(scope.all_judge_all());
}

#[test]
fn test_a_mutant_in_no_package_makes_the_whole_workspace_judge() {
    let plans = [
        plan_in("m1", "core/src/lib.rs"),
        plan_in("m2", "stray/lib.rs"),
    ];
    let scope = Scope::of(&TestPackages::Mutated, &plans, &packages(), Path::new("/w"));
    assert_eq!(scope.build(), None);
    assert!(scope.all_judge_all());
    assert!(
        scope.mutated().is_empty(),
        "the check then compiles the workspace"
    );
}

#[test]
fn test_no_planned_mutant_builds_no_program() {
    let scope = Scope::of(&TestPackages::Mutated, &[], &packages(), Path::new("/w"));
    assert_eq!(scope.build(), Some(Vec::new()));
}

fn loaded(config: &str, args: SelectArgs) -> Selection {
    let dir = std::env::temp_dir().join(format!(
        "cherenkov-scope-{}-{}",
        std::process::id(),
        config.len()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("mutants.toml");
    std::fs::write(&file, config).unwrap();
    let args = SelectArgs {
        config: Some(file),
        ..args
    };
    let selection = Selection::load(&dir, &args, &[]).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    selection
}

#[test]
fn test_the_configuration_s_test_workspace_selects_the_whole_workspace() {
    let selection = loaded("test_workspace = true\n", SelectArgs::default());
    assert_eq!(selection.test_packages, TestPackages::Workspace);
}

#[test]
fn test_a_command_line_test_workspace_false_overrides_the_configuration_s_true() {
    let args = SelectArgs {
        test_workspace: Some(false),
        ..SelectArgs::default()
    };
    let selection = loaded("test_workspace = true\n", args);
    assert_eq!(selection.test_packages, TestPackages::Mutated);
}

#[test]
fn test_test_package_is_a_list_of_packages_to_judge_by_and_not_an_argument_for_cargo() {
    let args = SelectArgs {
        test_package: named(&["a", "b"]),
        ..SelectArgs::default()
    };
    let selection = loaded("additional_cargo_args = [\"--locked\"]\n", args);
    assert_eq!(
        selection.test_packages,
        TestPackages::Named(named(&["a", "b"]))
    );
    assert_eq!(selection.cargo_args, ["--locked"]);
}
