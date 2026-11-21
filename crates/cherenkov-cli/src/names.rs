//! A planned mutant's name as cargo-mutants spells it, `path:line:col:
//! replace X with Y in Function`, so regular expressions written for
//! cargo-mutants (`--exclude-re`, `exclude_re` in `mutants.toml`) match the
//! same mutants here. Only Rust sources have these names; a mutant of any
//! other language is named `path:line:col: replace X with Y`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cherenkov_core::plan::MutantPlan;
use cherenkov_syntax::rust_names::{Names, arm_pattern, squash_lines};

/// The name of each of `plans`, in order. `root` is where their files are
/// read from; a file that cannot be read leaves its mutants unnamed in
/// the function context.
pub fn name_all(root: &Path, plans: &[MutantPlan]) -> Vec<String> {
    let mut files: BTreeMap<PathBuf, Option<(String, Names)>> = BTreeMap::new();
    plans
        .iter()
        .map(|plan| {
            let file = files.entry(plan.file.clone()).or_insert_with(|| {
                let source = std::fs::read_to_string(root.join(&plan.file)).ok()?;
                let names = Names::parse(&source);
                Some((source, names))
            });
            name_of(plan, file.as_ref().map(|(s, n)| (s.as_str(), n)))
        })
        .collect()
}

/// `plan`'s name, given its file's text and names.
fn name_of(plan: &MutantPlan, file: Option<(&str, &Names)>) -> String {
    let path = plan.file.to_string_lossy().replace('\\', "/");
    let (line, col) = (plan.site.start.line, char_column(plan, file));
    format!("{path}:{line}:{col}: {}", describe(plan, file))
}

/// The 1-based column in characters, which cargo-mutants counts; the
/// planner's is in bytes.
fn char_column(plan: &MutantPlan, file: Option<(&str, &Names)>) -> usize {
    let (Some((source, _)), start) = (file, plan.site.start) else {
        return plan.site.start.col as usize;
    };
    let line_start = start.byte.saturating_sub(start.col as usize - 1);
    source
        .get(line_start..start.byte)
        .map_or(start.col as usize, |before| before.chars().count() + 1)
}

fn describe(plan: &MutantPlan, file: Option<(&str, &Names)>) -> String {
    let function =
        file.and_then(|(_, names)| names.function_at(plan.site.start.byte, plan.site.end.byte));
    let in_function = function
        .map(|f| format!(" in {}", f.qualified))
        .unwrap_or_default();
    let with = plan.mutated.as_deref().unwrap_or_default();
    match plan.operator {
        "fn-value" => {
            let f = function.map(|f| f.qualified.as_str()).unwrap_or_default();
            let ret = function
                .map(|f| f.return_type.as_str())
                .filter(|r| !r.is_empty())
                .map(|r| format!(" {r}"))
                .unwrap_or_default();
            format!("replace {f}{ret} with {with}")
        }
        "match-arm" => format!(
            "delete match arm {}{in_function}",
            squash_lines(&arm_pattern(&plan.original))
        ),
        "match-guard" => format!(
            "replace match guard {} with {with}{in_function}",
            squash_lines(&plan.original)
        ),
        "struct-field" => {
            let field = plan.original.split(':').next().unwrap_or_default();
            let field = field.trim().trim_end_matches(',');
            let owner = file
                .and_then(|(_, names)| names.struct_at(plan.site.start.byte, plan.site.end.byte))
                .unwrap_or_default();
            format!("delete field {field} from struct {owner} expression{in_function}")
        }
        _ if with.is_empty() => format!("delete {}{in_function}", plan.original),
        _ => format!("replace {} with {with}{in_function}", plan.original),
    }
}
