use super::*;
use crate::changed_tests::ChangedTest;
use std::path::PathBuf;

use crate::selection::{SelectArgs, Selection};

pub(crate) fn plan_in(id: &str, file: &str) -> MutantPlan {
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
            deps: deps_of(n),
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
    for package in ["core", "util", "app"] {
        assert_eq!(ids(&scope, package), ["m1"], "{package}");
    }
}

#[test]
fn test_a_mutant_in_no_package_makes_the_whole_workspace_judge() {
    let plans = [
        plan_in("m1", "core/src/lib.rs"),
        plan_in("m2", "stray/lib.rs"),
    ];
    let scope = Scope::of(&TestPackages::Mutated, &plans, &packages(), Path::new("/w"));
    assert_eq!(scope.build(), None);
    assert_eq!(ids(&scope, "util"), ["m1", "m2"]);
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

/// app depends on core; util on nothing.
fn deps_of(name: &str) -> Vec<String> {
    match name {
        "app" => vec!["core".to_string(), "util".to_string()],
        "util" => vec!["core".to_string()],
        _ => Vec::new(),
    }
}

fn changed(file: &str, path: &str) -> ChangedTest {
    ChangedTest {
        file: PathBuf::from(file),
        path: path.to_string(),
    }
}

#[test]
fn test_a_package_reaches_itself_and_its_workspace_dependencies_transitively() {
    let mut reached: Vec<_> = reach(&packages(), "app").into_iter().collect();
    reached.sort();
    assert_eq!(reached, ["app", "core", "util"]);
    let mut reached: Vec<_> = reach(&packages(), "util").into_iter().collect();
    reached.sort();
    assert_eq!(reached, ["core", "util"]);
    assert_eq!(reach(&packages(), "core").len(), 1);
}

#[test]
fn test_a_changed_test_judges_the_mutants_of_every_package_its_package_reaches() {
    let app_plans = [plan_in("m1", "app/src/lib.rs")];
    let all = [
        plan_in("m1", "app/src/lib.rs"),
        plan_in("m2", "core/src/lib.rs"),
        plan_in("m3", "util/src/lib.rs"),
    ];
    let scope = Scope::of(
        &TestPackages::Mutated,
        &app_plans,
        &packages(),
        Path::new("/w"),
    )
    .with_changed_tests(
        &[changed("util/src/lib.rs", "tests::t")],
        &all,
        &packages(),
        Path::new("/w"),
    );
    // util reaches util and core: m2, m3 are judged by util's tests, and by
    // the changed ones alone; m1 (app's, changed code) is not util's.
    assert_eq!(ids(&scope, "util"), ["m2", "m3"]);
    assert_eq!(scope.only_changed("util").len(), 2);
    assert_eq!(ids(&scope, "app"), ["m1"]);
    assert!(scope.only_changed("app").is_empty());
    assert_eq!(scope.build(), Some(named(&["app", "util"])));
    assert_eq!(
        scope.changed_in("util"),
        [changed("util/src/lib.rs", "tests::t")]
    );
    assert_eq!(scope.mutated(), named(&["app", "core", "util"]));
}

#[test]
fn test_a_mutant_changed_application_code_and_reached_by_a_changed_test_runs_every_test() {
    let plans = [plan_in("m1", "core/src/lib.rs")];
    let scope = Scope::of(&TestPackages::Mutated, &plans, &packages(), Path::new("/w"))
        .with_changed_tests(
            &[changed("core/src/lib.rs", "tests::t")],
            &plans,
            &packages(),
            Path::new("/w"),
        );
    assert_eq!(ids(&scope, "core"), ["m1"]);
    assert!(
        scope.only_changed("core").is_empty(),
        "not only the changed tests"
    );
}

#[test]
fn test_a_changed_test_judges_alone_a_mutant_of_changed_code_in_a_package_it_reaches() {
    // m1 is changed code in core; util's tests do not judge it by default,
    // so util's changed test does, and alone.
    let plans = [
        plan_in("m1", "core/src/lib.rs"),
        plan_in("m2", "util/src/lib.rs"),
    ];
    let app = [plan_in("m1", "core/src/lib.rs")];
    let scope = Scope::of(&TestPackages::Mutated, &app, &packages(), Path::new("/w"))
        .with_changed_tests(
            &[changed("util/src/lib.rs", "tests::t")],
            &plans,
            &packages(),
            Path::new("/w"),
        );
    assert_eq!(ids(&scope, "core"), ["m1"]);
    assert!(scope.only_changed("core").is_empty());
    let only: Vec<_> = scope
        .only_changed("util")
        .iter()
        .map(|i| i.0.clone())
        .collect();
    assert_eq!(only, ["m1", "m2"]);
    let all: Vec<_> = scope
        .judged_for_changed_tests()
        .into_iter()
        .map(|i| i.0)
        .collect();
    assert_eq!(all, ["m1", "m2"]);
}

#[test]
fn test_without_changed_tests_nothing_is_judged_for_them() {
    let plans = [plan_in("m1", "core/src/lib.rs")];
    let scope = Scope::of(&TestPackages::Mutated, &plans, &packages(), Path::new("/w"));
    assert!(scope.judged_for_changed_tests().is_empty());
}

fn set(list: &[&str]) -> BTreeSet<MutantId> {
    list.iter().map(|i| MutantId((*i).into())).collect()
}

#[test]
fn test_the_plans_to_run_are_changed_code_s_and_what_changed_tests_reach_once_each() {
    let every = [
        plan_in("m1", "core/src/lib.rs"),
        plan_in("m2", "util/src/lib.rs"),
        plan_in("m3", "app/src/lib.rs"),
    ];
    let code = [every[0].clone()];
    let scope = Scope::of(&TestPackages::Mutated, &code, &packages(), Path::new("/w"))
        .with_changed_tests(
            &[changed("util/src/lib.rs", "tests::t")],
            &every[..2],
            &packages(),
            Path::new("/w"),
        );
    let run: Vec<_> = scope
        .plans_to_run(code.to_vec(), &every)
        .into_iter()
        .map(|p| p.id.0)
        .collect();
    assert_eq!(run, ["m1", "m2"], "m3 is reached by no changed test");
}

fn yes(
    judged: &BTreeSet<MutantId>,
    only: &BTreeSet<MutantId>,
    tests: &[String],
    user: &[String],
) -> bool {
    records_changed_tests_only(judged, only, tests, user)
}

#[test]
fn test_a_program_records_the_changed_tests_alone_only_when_nothing_else_is_judged_there() {
    let only = set(&["m2"]);
    let tests = vec!["t".to_string()];
    assert!(yes(&set(&["m2"]), &only, &tests, &[]));
    assert!(
        !yes(&set(&["m1", "m2"]), &only, &tests, &[]),
        "changed code is judged here too"
    );
    assert!(
        !yes(&set(&[]), &set(&[]), &tests, &[]),
        "no changed-test mutant"
    );
    assert!(
        !yes(&set(&["m2"]), &only, &[], &[]),
        "no changed test in this program"
    );
    assert!(
        !yes(&set(&["m2"]), &only, &tests, &["u".to_string()]),
        "the user chose the tests"
    );
}
